//! Bubblewrap (bwrap) isolation backend.
//!
//! Unlike the container-engine path, bwrap does not build or pull an image.
//! The agent binary must already be installed on the host.  Bwrap creates a
//! lightweight mount namespace that gives the agent a restricted view of the
//! filesystem while keeping network access and the current user's identity.
//!
//! `build_command` is the authoritative description of the sandbox layout.

use std::fs;
use std::process::{Command, ExitStatus};

use crate::cli::Harness;
use crate::docker::RunConfig;

/// Run the agent under bubblewrap.  No image build step.
pub fn run(cfg: &RunConfig) -> Result<(), String> {
    let cmd = build_command(cfg)?;

    if cfg.dry_run {
        print_dry_run(&cmd);
        return Ok(());
    }

    if !cfg.verbose {
        let label = match cfg.harness {
            Harness::Pi => "pi",
            Harness::Claude => "claude",
            Harness::Codex => "codex",
        };
        println!(
            "{}",
            crate::docker::banner(&format!("{label} (bwrap)"), &cfg.presets)
        );
    }

    exec(&cmd)
}

fn build_command(cfg: &RunConfig) -> Result<Vec<String>, String> {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/root".to_string());
    let mut cmd = vec![s("bwrap")];

    // Read-only system paths.  /usr always exists; everything else is
    // --ro-bind-try to handle distros where /bin is a symlink into /usr.
    ro_bind(&mut cmd, "/usr");
    for path in [
        "/lib", "/lib32", "/lib64", "/libx32", "/bin", "/sbin", "/opt",
    ] {
        ro_bind_try(&mut cmd, path);
    }

    // Virtual filesystems.
    cmd.extend([s("--proc"), s("/proc")]);
    cmd.extend([s("--dev"), s("/dev")]);
    cmd.extend([s("--tmpfs"), s("/tmp")]);
    cmd.extend([s("--tmpfs"), s("/run")]);

    // Create the home directory so subsequent bind mounts beneath it have a
    // parent directory to attach to.
    cmd.extend([s("--dir"), home.clone()]);

    // Agent config directories (created on the host if absent, then rw-mounted).
    for path in agent_config_paths(cfg, &home)? {
        cmd.extend([s("--bind"), path.clone(), path]);
    }

    // User volumes (rw).
    for (host, container) in &cfg.volumes {
        cmd.extend([s("--bind"), host.clone(), container.clone()]);
    }

    // Shadow volumes (ro overlay).
    for (source, container) in &cfg.shadow_volumes {
        cmd.extend([s("--ro-bind"), source.clone(), container.clone()]);
    }

    // Minimal /etc subset for networking and user identity.
    for path in [
        "/etc/resolv.conf",
        "/etc/hosts",
        "/etc/nsswitch.conf",
        "/etc/passwd",
        "/etc/group",
        "/etc/ssl",
        "/etc/ca-certificates",
        "/etc/pki",
        "/etc/gai.conf",
    ] {
        ro_bind_try(&mut cmd, path);
    }

    // Namespace isolation: unshare everything except the network (agents need
    // the internet).
    cmd.push(s("--unshare-all"));
    cmd.push(s("--share-net"));
    cmd.push(s("--die-with-parent"));

    // Working directory.
    cmd.extend([s("--chdir"), cfg.workdir.clone()]);

    // Environment.  Propagate the host PATH verbatim so the agent binary is
    // found regardless of where it was installed (e.g. /opt/pi-bun/bin).
    setenv(&mut cmd, "HOME", &home);
    setenv(
        &mut cmd,
        "PATH",
        &std::env::var("PATH").unwrap_or_else(|_| "/usr/local/bin:/usr/bin:/bin".to_string()),
    );

    for (key, val) in &cfg.env_vars {
        setenv(&mut cmd, key, val);
    }
    for key in ["ANTHROPIC_API_KEY", "OPENAI_API_KEY", "OPEN_ROUTER_KEY"] {
        setenv(&mut cmd, key, &std::env::var(key).unwrap_or_default());
    }

    // Resolve the agent binary and bind every directory it needs to exec:
    // the launcher itself, the script it points at, and that script's
    // dependency tree.  Paths already covered by /usr, /opt, etc. are skipped.
    let binary = resolve_binary(cfg)?;
    for path in binary_mount_paths(&binary) {
        if !is_path_covered(&path) {
            ro_bind(&mut cmd, &path);
        }
    }

    // Agent binary then forwarded arguments.
    cmd.push(binary);
    cmd.extend(cfg.container_args.iter().cloned());

    Ok(cmd)
}

/// Returns the list of host paths to bind-mount read-write for agent state.
/// Creates each path on the host if it does not yet exist.
fn agent_config_paths(cfg: &RunConfig, home: &str) -> Result<Vec<String>, String> {
    let mut paths: Vec<String> = Vec::new();

    match cfg.harness {
        Harness::Pi => {
            let pi_dir = format!("{home}/.pi");
            fs::create_dir_all(&pi_dir).map_err(|e| format!("failed to create {pi_dir}: {e}"))?;
            paths.push(pi_dir);
        }
        Harness::Claude => {
            let claude_dir = format!("{home}/.claude");
            fs::create_dir_all(&claude_dir)
                .map_err(|e| format!("failed to create {claude_dir}: {e}"))?;
            paths.push(claude_dir);

            // claude.json is a file; create it if absent before mounting.
            let claude_json = format!("{home}/.claude.json");
            if !std::path::Path::new(&claude_json).exists() {
                fs::write(&claude_json, "{}")
                    .map_err(|e| format!("failed to create {claude_json}: {e}"))?;
            }
            paths.push(claude_json);
        }
        Harness::Codex => {
            let codex_dir = format!("{home}/.codex");
            fs::create_dir_all(&codex_dir)
                .map_err(|e| format!("failed to create {codex_dir}: {e}"))?;
            paths.push(codex_dir);
        }
    }

    Ok(paths)
}

/// Resolve the absolute path to the agent binary.
///
/// Uses `cfg.harness_binary` when explicitly configured; otherwise searches
/// the host PATH.  Returns a clear error if the binary cannot be found,
/// directing the user to install it or set the config option.
fn resolve_binary(cfg: &RunConfig) -> Result<String, String> {
    let path_env = std::env::var("PATH").unwrap_or_default();
    resolve_binary_in(cfg, &path_env)
}

/// `resolve_binary` with the search path injected, so tests can exercise the
/// not-found path without mutating the process environment.
fn resolve_binary_in(cfg: &RunConfig, path_env: &str) -> Result<String, String> {
    let name = match cfg.harness {
        Harness::Pi => "pi",
        Harness::Claude => "claude",
        Harness::Codex => "codex",
    };

    if let Some(ref path) = cfg.harness_binary {
        if std::path::Path::new(path).is_file() {
            return Ok(path.clone());
        }
        return Err(format!("configured {name}-path does not exist: {path}"));
    }

    search_path(name, path_env).ok_or_else(|| {
        let install_hint = match cfg.harness {
            Harness::Pi => "\n  Install with: bun install -g @earendil-works/pi-coding-agent",
            Harness::Claude | Harness::Codex => "",
        };
        format!(
            "`{name}` not found in PATH.{install_hint}\n  \
             Or set {name}-path in ~/.config/orka/config.yaml"
        )
    })
}

/// Search the supplied `PATH`-style string for `name`, returning its absolute
/// path.  The search path is passed in rather than read from the environment
/// so callers (and tests) can inject one without touching the process
/// environment.
fn search_path(name: &str, path_env: &str) -> Option<String> {
    std::env::split_paths(path_env)
        .map(|dir| dir.join(name))
        .find(|p| p.is_file())
        .map(|p| p.to_string_lossy().into_owned())
}

/// Directories that must be visible inside the sandbox for `binary` to exec.
///
/// A launcher found on PATH is rarely a self-contained executable.  npm, bun
/// and mise all install `<prefix>/bin/<cmd>` as a *relative symlink* into
/// `<prefix>/lib/node_modules/<pkg>/<entry>.js`, and that script needs its
/// sibling dependency tree at runtime.  Binding only the launcher's own
/// directory leaves the symlink dangling inside the sandbox, so execvp
/// reports ENOENT for a path that plainly exists on the host.  Given
/// `PREFIX/bin/pi` symlinked to `../lib/node_modules/@scope/pkg/dist/index.js`,
/// both `PREFIX/bin` (the launcher) and `PREFIX/lib` (the package tree, which
/// covers the entry script and its dependencies) have to be bound.
///
/// The shebang interpreter is bound too, since a missing interpreter produces
/// the identical ENOENT.  Result is deduplicated with subsumed paths removed.
fn binary_mount_paths(binary: &str) -> Vec<String> {
    let mut paths: Vec<String> = Vec::new();

    if let Some(dir) = parent_dir(binary) {
        paths.push(dir);
    }

    if let Ok(target) = fs::canonicalize(binary) {
        let target = target.to_string_lossy().into_owned();

        match node_package_root(&target) {
            Some(root) => paths.push(root),
            None => {
                if let Some(dir) = parent_dir(&target) {
                    paths.push(dir);
                }
            }
        }

        if let Some(dir) = shebang_interpreter(&target).as_deref().and_then(parent_dir) {
            paths.push(dir);
        }
    }

    normalize_mount_paths(paths)
}

fn parent_dir(path: &str) -> Option<String> {
    std::path::Path::new(path)
        .parent()
        .map(|p| p.to_string_lossy().into_owned())
        .filter(|p| !p.is_empty())
}

/// For a path inside an installed npm package, the directory holding the
/// outermost `node_modules`.  Binding it exposes the entry script together
/// with every dependency it can require.
fn node_package_root(path: &str) -> Option<String> {
    let idx = path.find("/node_modules/")?;
    let root = &path[..idx];
    (!root.is_empty()).then(|| root.to_string())
}

/// The interpreter from a `#!` line, if `path` is a script.
fn shebang_interpreter(path: &str) -> Option<String> {
    use std::io::Read;

    let mut head = [0u8; 256];
    let read = fs::File::open(path).ok()?.read(&mut head).ok()?;
    let head = head.get(..read)?;
    if !head.starts_with(b"#!") {
        return None;
    }

    let line = match head.iter().position(|&b| b == b'\n') {
        Some(end) => &head[2..end],
        None => &head[2..],
    };
    String::from_utf8_lossy(line)
        .split_whitespace()
        .next()
        .map(str::to_string)
}

/// Keep only existing directories, in a stable order, dropping duplicates and
/// any path already contained in another entry.
fn normalize_mount_paths(mut paths: Vec<String>) -> Vec<String> {
    paths.retain(|p| p != "/" && std::path::Path::new(p).is_dir());
    paths.sort();
    paths.dedup();

    // Sorted ascending, an ancestor always precedes its descendants.
    let mut kept: Vec<String> = Vec::new();
    for path in paths {
        if !kept.iter().any(|k| is_under(&path, k)) {
            kept.push(path);
        }
    }
    kept
}

fn is_under(path: &str, ancestor: &str) -> bool {
    path.strip_prefix(ancestor)
        .is_some_and(|rest| rest.starts_with('/'))
}

/// Returns true when `path` is under a directory that is already bind-mounted
/// read-only by the standard bwrap setup (so no additional mount is needed).
fn is_path_covered(path: &str) -> bool {
    // /lib covers /lib32, /lib64, /libx32 as they all start with "/lib".
    ["/usr/", "/lib", "/bin", "/sbin", "/opt/"]
        .iter()
        .any(|prefix| path.starts_with(prefix))
}

// ---------------------------------------------------------------------------
// bwrap argument helpers
// ---------------------------------------------------------------------------

fn ro_bind(cmd: &mut Vec<String>, path: &str) {
    cmd.extend([s("--ro-bind"), s(path), s(path)]);
}

fn ro_bind_try(cmd: &mut Vec<String>, path: &str) {
    cmd.extend([s("--ro-bind-try"), s(path), s(path)]);
}

fn setenv(cmd: &mut Vec<String>, key: &str, val: &str) {
    cmd.extend([s("--setenv"), s(key), s(val)]);
}

// ---------------------------------------------------------------------------
// Execution helpers
// ---------------------------------------------------------------------------

fn exec(args: &[String]) -> Result<(), String> {
    let (program, rest) = args
        .split_first()
        .ok_or_else(|| "empty command".to_string())?;

    let status: ExitStatus = Command::new(program)
        .args(rest)
        .status()
        .map_err(|e| format!("failed to launch `{program}`: {e}"))?;

    if status.success() {
        Ok(())
    } else {
        Err(format!(
            "`{program}` exited with status {}",
            status.code().unwrap_or(-1)
        ))
    }
}

fn print_dry_run(args: &[String]) {
    println!("[DRY_RUN] bwrap run:");
    let line = args
        .iter()
        .enumerate()
        .map(|(i, a)| {
            // Redact the VALUE token of --setenv KEY VALUE.
            if i > 1 && args[i - 2] == "--setenv" {
                return s("<redacted>");
            }
            shell_quote(a)
        })
        .collect::<Vec<_>>()
        .join(" \\\n  ");
    println!("  {line}");
    println!();
}

fn shell_quote(val: &str) -> String {
    if val.chars().any(|c| {
        matches!(
            c,
            ' ' | '\t'
                | '\n'
                | '"'
                | '\''
                | '\\'
                | '$'
                | '!'
                | '&'
                | '|'
                | ';'
                | '('
                | ')'
                | '<'
                | '>'
        )
    }) {
        format!("'{}'", val.replace('\'', "'\\''"))
    } else {
        val.to_owned()
    }
}

fn s(lit: &str) -> String {
    lit.to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    // Creates a RunConfig with a real temp binary file so resolve_binary
    // succeeds without needing the agent installed on the test machine.
    // Returns the TempDir too — caller must keep it alive for the test.
    fn make_cfg(harness: Harness) -> (RunConfig, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let name = match harness {
            Harness::Pi => "pi",
            Harness::Claude => "claude",
            Harness::Codex => "codex",
        };
        let binary = dir.path().join(name);
        fs::write(&binary, "").unwrap();
        let cfg = RunConfig {
            engine_binary: s("bwrap"),
            harness,
            no_cache: false,
            dry_run: false,
            verbose: false,
            preserve_container: false,
            harness_version: None,
            harness_binary: Some(binary.to_string_lossy().into_owned()),
            backend: crate::cli::Backend::Bubblewrap,
            quiet: false,
            volumes: vec![],
            shadow_volumes: vec![],
            env_vars: vec![],
            presets: vec![],
            workdir: "/work".to_string(),
            container_args: vec![],
        };
        (cfg, dir)
    }

    #[test]
    fn command_starts_with_bwrap() {
        let (cfg, _dir) = make_cfg(Harness::Pi);
        let cmd = build_command(&cfg).unwrap();
        assert_eq!(cmd[0], "bwrap");
    }

    #[test]
    fn command_unshares_all_but_keeps_net() {
        let (cfg, _dir) = make_cfg(Harness::Pi);
        let cmd = build_command(&cfg).unwrap();
        let joined = cmd.join(" ");
        assert!(joined.contains("--unshare-all"));
        assert!(joined.contains("--share-net"));
    }

    #[test]
    fn usr_is_ro_bind() {
        let (cfg, _dir) = make_cfg(Harness::Pi);
        let cmd = build_command(&cfg).unwrap();
        let pos = cmd
            .windows(3)
            .find(|w| w[0] == "--ro-bind" && w[1] == "/usr" && w[2] == "/usr");
        assert!(pos.is_some(), "/usr must appear as --ro-bind");
    }

    #[test]
    fn workdir_is_chdir_target() {
        let (cfg, _dir) = make_cfg(Harness::Pi);
        let cmd = build_command(&cfg).unwrap();
        let idx = cmd.iter().position(|a| a == "--chdir").unwrap();
        assert_eq!(cmd[idx + 1], "/work");
    }

    #[test]
    fn binary_is_last_token_pi() {
        let (cfg, _dir) = make_cfg(Harness::Pi);
        let cmd = build_command(&cfg).unwrap();
        assert!(cmd.last().unwrap().ends_with("/pi"));
    }

    #[test]
    fn binary_is_last_token_claude() {
        let (cfg, _dir) = make_cfg(Harness::Claude);
        let cmd = build_command(&cfg).unwrap();
        assert!(cmd.last().unwrap().ends_with("/claude"));
    }

    #[test]
    fn binary_is_last_token_codex() {
        let (cfg, _dir) = make_cfg(Harness::Codex);
        let cmd = build_command(&cfg).unwrap();
        assert!(cmd.last().unwrap().ends_with("/codex"));
    }

    #[test]
    fn container_args_forwarded_after_binary() {
        let (mut cfg, _dir) = make_cfg(Harness::Pi);
        cfg.container_args = vec!["do the thing".to_string()];
        let cmd = build_command(&cfg).unwrap();
        assert_eq!(cmd.last().unwrap(), "do the thing");
        assert!(cmd[cmd.len() - 2].ends_with("/pi"));
    }

    #[test]
    fn user_volumes_use_bind() {
        let (mut cfg, _dir) = make_cfg(Harness::Pi);
        cfg.volumes = vec![("/home/user/proj".to_string(), "/home/user/proj".to_string())];
        let cmd = build_command(&cfg).unwrap();
        let found = cmd
            .windows(3)
            .any(|w| w[0] == "--bind" && w[1] == "/home/user/proj" && w[2] == "/home/user/proj");
        assert!(found);
    }

    #[test]
    fn shadow_volumes_use_ro_bind() {
        let (mut cfg, _dir) = make_cfg(Harness::Pi);
        cfg.shadow_volumes = vec![("/tmp/empty".to_string(), "/project/.env".to_string())];
        let cmd = build_command(&cfg).unwrap();
        let found = cmd
            .windows(3)
            .any(|w| w[0] == "--ro-bind" && w[1] == "/tmp/empty" && w[2] == "/project/.env");
        assert!(found, "shadow volume should appear as --ro-bind");
    }

    #[test]
    fn env_vars_use_setenv() {
        let (mut cfg, _dir) = make_cfg(Harness::Pi);
        cfg.env_vars = vec![("MY_VAR".to_string(), "hello".to_string())];
        let cmd = build_command(&cfg).unwrap();
        let found = cmd
            .windows(3)
            .any(|w| w[0] == "--setenv" && w[1] == "MY_VAR" && w[2] == "hello");
        assert!(found);
    }

    #[test]
    fn resolve_binary_uses_configured_path() {
        let dir = tempfile::tempdir().unwrap();
        let binary = dir.path().join("pi");
        fs::write(&binary, "").unwrap();
        let mut cfg = make_cfg(Harness::Pi).0;
        cfg.harness_binary = Some(binary.to_string_lossy().into_owned());
        let result = resolve_binary(&cfg).unwrap();
        assert_eq!(result, binary.to_string_lossy());
    }

    #[test]
    fn resolve_binary_errors_on_missing_configured_path() {
        let mut cfg = make_cfg(Harness::Pi).0;
        cfg.harness_binary = Some("/nonexistent/pi".to_string());
        let err = resolve_binary(&cfg).unwrap_err();
        assert!(err.contains("does not exist"));
    }

    #[test]
    fn resolve_binary_errors_when_not_in_path() {
        // Use a harness name that is guaranteed not to be in PATH.
        let mut cfg = make_cfg(Harness::Pi).0;
        cfg.harness_binary = None;
        // Inject an empty directory as the search path so nothing is found,
        // without mutating the process-global PATH (a data race under parallel
        // tests, and unsafe as of edition 2024).
        let empty = tempfile::tempdir().unwrap();
        let path_env = empty.path().to_string_lossy();
        let err = resolve_binary_in(&cfg, &path_env).unwrap_err();
        assert!(err.contains("not found in PATH"));
        assert!(err.contains("pi-path"));
    }

    #[test]
    fn is_path_covered_usr() {
        assert!(is_path_covered("/usr/local/bin/pi"));
        assert!(is_path_covered("/usr/bin/pi"));
    }

    #[test]
    fn is_path_covered_opt() {
        assert!(is_path_covered("/opt/pi-bun/bin/pi"));
    }

    #[test]
    fn is_path_covered_home_is_not() {
        assert!(!is_path_covered("/home/user/.bun/bin/pi"));
        assert!(!is_path_covered("/root/.bun/bin/pi"));
    }

    #[test]
    fn uncovered_binary_gets_extra_ro_bind() {
        // Binary is in a temp dir (not under /usr, /opt, etc.).
        // build_command should emit an --ro-bind for that directory.
        let (cfg, _dir) = make_cfg(Harness::Pi);
        let binary = cfg.harness_binary.as_ref().unwrap().clone();
        let binary_dir = std::path::Path::new(&binary)
            .parent()
            .unwrap()
            .to_string_lossy()
            .into_owned();
        assert!(!is_path_covered(&binary_dir));
        let cmd = build_command(&cfg).unwrap();
        assert!(
            has_bind(&cmd, "--ro-bind", &binary_dir),
            "binary dir should get --ro-bind when not already covered"
        );
    }

    fn has_bind(cmd: &[String], flag: &str, path: &str) -> bool {
        cmd.windows(3)
            .any(|w| w[0] == flag && w[1] == path && w[2] == path)
    }

    /// Reproduces the npm/mise global install layout:
    ///
    ///   <prefix>/bin/pi -> ../lib/node_modules/@scope/pkg/dist/index.js
    ///   <prefix>/lib/node_modules/@scope/pkg/node_modules/dep/...
    ///
    /// Returns (tempdir, prefix, launcher path).
    fn npm_style_install(shebang: &str) -> (tempfile::TempDir, String, String) {
        let dir = tempfile::tempdir().unwrap();
        let prefix = dir.path().join("node/24.11.1");
        let bin = prefix.join("bin");
        let pkg = prefix.join("lib/node_modules/@earendil-works/pi-coding-agent");
        let dist = pkg.join("dist");
        fs::create_dir_all(&bin).unwrap();
        fs::create_dir_all(&dist).unwrap();
        fs::create_dir_all(pkg.join("node_modules/some-dep")).unwrap();

        fs::write(
            dist.join("index.js"),
            format!("{shebang}\nconsole.log(1);\n"),
        )
        .unwrap();

        let launcher = bin.join("pi");
        std::os::unix::fs::symlink(
            "../lib/node_modules/@earendil-works/pi-coding-agent/dist/index.js",
            &launcher,
        )
        .unwrap();

        let prefix = prefix.to_string_lossy().into_owned();
        let launcher = launcher.to_string_lossy().into_owned();
        (dir, prefix, launcher)
    }

    /// The regression: binding only the launcher directory leaves the relative
    /// symlink dangling in the sandbox and execvp fails with ENOENT.  The
    /// package tree root must be bound as well.
    #[test]
    fn npm_symlink_launcher_binds_package_tree_root() {
        let (_tmp, prefix, launcher) = npm_style_install("#!/usr/bin/env node");
        let paths = binary_mount_paths(&launcher);

        assert!(
            paths.contains(&format!("{prefix}/bin")),
            "launcher dir must be bound: {paths:?}"
        );
        assert!(
            paths.contains(&format!("{prefix}/lib")),
            "package tree root must be bound so the symlink target and its \
             dependencies resolve: {paths:?}"
        );
    }

    /// The whole point of binding the package root: the symlink must resolve
    /// to a real file through one of the returned mounts.
    #[test]
    fn npm_symlink_target_lies_under_a_returned_mount() {
        let (_tmp, _prefix, launcher) = npm_style_install("#!/usr/bin/env node");
        let target = fs::canonicalize(&launcher).unwrap();
        let target = target.to_string_lossy().into_owned();
        let paths = binary_mount_paths(&launcher);
        assert!(
            paths.iter().any(|p| is_under(&target, p)),
            "symlink target {target} is not covered by any mount: {paths:?}"
        );
    }

    /// Dependencies sit in a nested node_modules; the outermost one defines the
    /// root, so nested trees stay covered by a single mount.
    #[test]
    fn nested_node_modules_resolve_to_outermost_root() {
        assert_eq!(
            node_package_root("/p/lib/node_modules/@s/pkg/node_modules/dep/i.js"),
            Some("/p/lib".to_string())
        );
    }

    #[test]
    fn node_package_root_is_none_outside_node_modules() {
        assert_eq!(node_package_root("/opt/pi-bun/bin/pi"), None);
    }

    /// A launcher installed under $HOME must still exec: build_command has to
    /// emit binds for both the bin dir and the package tree.
    #[test]
    fn build_command_binds_package_tree_for_home_install() {
        let (_tmp, prefix, launcher) = npm_style_install("#!/usr/bin/env node");
        let (mut cfg, _dir) = make_cfg(Harness::Pi);
        cfg.harness_binary = Some(launcher.clone());

        let cmd = build_command(&cfg).unwrap();
        assert!(has_bind(&cmd, "--ro-bind", &format!("{prefix}/bin")));
        assert!(has_bind(&cmd, "--ro-bind", &format!("{prefix}/lib")));
        assert_eq!(cmd.last().unwrap(), &launcher);
    }

    /// An absolute shebang interpreter outside the install prefix must be
    /// bound, otherwise exec fails with the same ENOENT.
    #[test]
    fn absolute_shebang_interpreter_dir_is_bound() {
        let node_dir = tempfile::tempdir().unwrap();
        let node = node_dir.path().join("node");
        fs::write(&node, "").unwrap();
        let shebang = format!("#!{}", node.to_string_lossy());

        let (_tmp, _prefix, launcher) = npm_style_install(&shebang);
        let paths = binary_mount_paths(&launcher);
        let expected = node_dir.path().to_string_lossy().into_owned();
        assert!(
            paths.contains(&expected),
            "interpreter dir {expected} must be bound: {paths:?}"
        );
    }

    #[test]
    fn env_shebang_needs_no_extra_mount() {
        assert_eq!(
            shebang_interpreter_of("#!/usr/bin/env node"),
            Some("/usr/bin/env".to_string())
        );
        assert!(is_path_covered("/usr/bin"));
    }

    #[test]
    fn shebang_with_arguments_takes_only_the_interpreter() {
        assert_eq!(
            shebang_interpreter_of("#!/usr/bin/node --experimental-vm-modules"),
            Some("/usr/bin/node".to_string())
        );
    }

    #[test]
    fn binary_without_shebang_yields_no_interpreter() {
        assert_eq!(shebang_interpreter_of("\x7fELF\x02\x01\x01"), None);
    }

    fn shebang_interpreter_of(contents: &str) -> Option<String> {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("prog");
        fs::write(&file, contents).unwrap();
        shebang_interpreter(&file.to_string_lossy())
    }

    #[test]
    fn normalize_drops_paths_subsumed_by_an_ancestor() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_string_lossy().into_owned();
        fs::create_dir_all(dir.path().join("a/b")).unwrap();

        let got = normalize_mount_paths(vec![
            format!("{root}/a/b"),
            root.clone(),
            format!("{root}/a"),
            root.clone(),
        ]);
        assert_eq!(got, vec![root]);
    }

    #[test]
    fn normalize_drops_nonexistent_and_root() {
        assert_eq!(
            normalize_mount_paths(vec![s("/"), s("/definitely/not/here")]),
            Vec::<String>::new()
        );
    }

    #[test]
    fn is_under_requires_a_path_boundary() {
        assert!(is_under("/a/b", "/a"));
        assert!(!is_under("/ab", "/a"));
        assert!(!is_under("/a", "/a"));
    }
}
