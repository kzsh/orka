//! Bubblewrap (bwrap) isolation backend.
//!
//! Unlike the container-engine path, bwrap does not build or pull an image.
//! The agent binary must already be installed on the host.  Bwrap creates a
//! lightweight mount namespace that gives the agent a restricted view of the
//! filesystem while keeping network access and the current user's identity.
//!
//! Sandbox filesystem layout:
//!
//!   /
//!   ├── usr/           ro-bind  (system binaries + libraries)
//!   ├── lib*/          ro-bind-try
//!   ├── bin/, sbin/    ro-bind-try
//!   ├── opt/           ro-bind-try  (common agent install prefix)
//!   ├── proc/          proc
//!   ├── dev/           dev
//!   ├── tmp/           tmpfs
//!   ├── run/           tmpfs
//!   ├── etc/
//!   │   ├── resolv.conf, hosts, ssl, ...  ro-bind-try
//!   └── home/<user>/
//!       ├── .pi/             bind rw  (pi config)
//!       ├── .claude/         bind rw  (claude config dir)
//!       ├── .claude.json     bind rw  (claude preferences)
//!       ├── .codex/          bind rw  (codex config)
//!       └── <workdir>        bind rw  (user project)

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
        println!("=====================");
        println!("Orka: {label} (bwrap)");
        println!("=====================");
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

    // Resolve the agent binary.  If its parent directory is not already
    // covered by a pre-mounted path (e.g. /usr, /opt), add an explicit
    // read-only bind mount for it.
    let binary = resolve_binary(cfg)?;
    let binary_dir = std::path::Path::new(&binary)
        .parent()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|| binary.clone());
    if !binary_dir.is_empty() && !is_path_covered(&binary_dir) {
        ro_bind_try(&mut cmd, &binary_dir);
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

    find_in_path(name).ok_or_else(|| {
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

/// Search the host PATH for a binary by name and return its absolute path.
fn find_in_path(name: &str) -> Option<String> {
    let path_env = std::env::var("PATH").unwrap_or_default();
    std::env::split_paths(&path_env)
        .map(|dir| dir.join(name))
        .find(|p| p.is_file())
        .map(|p| p.to_string_lossy().into_owned())
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
        // Override PATH to an empty directory so nothing is found.
        let empty = tempfile::tempdir().unwrap();
        let prev = std::env::var("PATH").ok();
        std::env::set_var("PATH", empty.path());
        let result = resolve_binary(&cfg);
        match prev {
            Some(v) => std::env::set_var("PATH", v),
            None => std::env::remove_var("PATH"),
        }
        let err = result.unwrap_err();
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
        // build_command should emit an --ro-bind-try for that directory.
        let (cfg, _dir) = make_cfg(Harness::Pi);
        let binary = cfg.harness_binary.as_ref().unwrap().clone();
        let binary_dir = std::path::Path::new(&binary)
            .parent()
            .unwrap()
            .to_string_lossy()
            .into_owned();
        assert!(!is_path_covered(&binary_dir));
        let cmd = build_command(&cfg).unwrap();
        let found = cmd
            .windows(3)
            .any(|w| w[0] == "--ro-bind-try" && w[1] == binary_dir && w[2] == binary_dir);
        assert!(
            found,
            "binary dir should get --ro-bind-try when not already covered"
        );
    }
}
