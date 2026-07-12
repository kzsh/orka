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
//!       ├── .agent-browser/  bind rw  (pi + browser)
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
    for path in ["/lib", "/lib32", "/lib64", "/libx32", "/bin", "/sbin", "/opt"] {
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
        &std::env::var("PATH")
            .unwrap_or_else(|_| "/usr/local/bin:/usr/bin:/bin".to_string()),
    );

    for (key, val) in &cfg.env_vars {
        setenv(&mut cmd, key, val);
    }
    for key in ["ANTHROPIC_API_KEY", "OPENAI_API_KEY", "OPEN_ROUTER_KEY"] {
        setenv(&mut cmd, key, &std::env::var(key).unwrap_or_default());
    }

    // Agent binary then forwarded arguments.
    cmd.push(agent_binary(cfg.harness));
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
            fs::create_dir_all(&pi_dir)
                .map_err(|e| format!("failed to create {pi_dir}: {e}"))?;
            paths.push(pi_dir);

            if !cfg.no_browser {
                let browser_dir = format!("{home}/.agent-browser");
                fs::create_dir_all(&browser_dir)
                    .map_err(|e| format!("failed to create {browser_dir}: {e}"))?;
                paths.push(browser_dir);
            }
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

/// Returns the agent binary name for the given harness.
fn agent_binary(harness: Harness) -> String {
    match harness {
        Harness::Pi => s("pi"),
        Harness::Claude => s("claude"),
        Harness::Codex => s("codex"),
    }
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

    fn make_cfg(harness: Harness) -> RunConfig {
        RunConfig {
            engine_binary: s("bwrap"),
            harness,
            no_cache: false,
            dry_run: false,
            verbose: false,
            preserve_container: false,
            harness_version: None,
            no_browser: true,
            volumes: vec![],
            shadow_volumes: vec![],
            env_vars: vec![],
            workdir: "/work".to_string(),
            container_args: vec![],
        }
    }

    #[test]
    fn command_starts_with_bwrap() {
        let cfg = make_cfg(Harness::Pi);
        let cmd = build_command(&cfg).unwrap();
        assert_eq!(cmd[0], "bwrap");
    }

    #[test]
    fn command_unshares_all_but_keeps_net() {
        let cfg = make_cfg(Harness::Pi);
        let cmd = build_command(&cfg).unwrap();
        let joined = cmd.join(" ");
        assert!(joined.contains("--unshare-all"));
        assert!(joined.contains("--share-net"));
    }

    #[test]
    fn usr_is_ro_bind() {
        let cfg = make_cfg(Harness::Pi);
        let cmd = build_command(&cfg).unwrap();
        let pos = cmd
            .windows(3)
            .find(|w| w[0] == "--ro-bind" && w[1] == "/usr" && w[2] == "/usr");
        assert!(pos.is_some(), "/usr must appear as --ro-bind");
    }

    #[test]
    fn workdir_is_chdir_target() {
        let cfg = make_cfg(Harness::Pi);
        let cmd = build_command(&cfg).unwrap();
        let idx = cmd.iter().position(|a| a == "--chdir").unwrap();
        assert_eq!(cmd[idx + 1], "/work");
    }

    #[test]
    fn pi_binary_is_last_token_without_args() {
        let cfg = make_cfg(Harness::Pi);
        let cmd = build_command(&cfg).unwrap();
        assert_eq!(cmd.last().unwrap(), "pi");
    }

    #[test]
    fn claude_binary_is_last_token_without_args() {
        let cfg = make_cfg(Harness::Claude);
        let cmd = build_command(&cfg).unwrap();
        assert_eq!(cmd.last().unwrap(), "claude");
    }

    #[test]
    fn codex_binary_is_last_token_without_args() {
        let cfg = make_cfg(Harness::Codex);
        let cmd = build_command(&cfg).unwrap();
        assert_eq!(cmd.last().unwrap(), "codex");
    }

    #[test]
    fn container_args_forwarded_after_binary() {
        let mut cfg = make_cfg(Harness::Pi);
        cfg.container_args = vec!["do the thing".to_string()];
        let cmd = build_command(&cfg).unwrap();
        assert_eq!(cmd.last().unwrap(), "do the thing");
        assert_eq!(cmd[cmd.len() - 2], "pi");
    }

    #[test]
    fn user_volumes_use_bind() {
        let mut cfg = make_cfg(Harness::Pi);
        cfg.volumes = vec![("/home/user/proj".to_string(), "/home/user/proj".to_string())];
        let cmd = build_command(&cfg).unwrap();
        let found = cmd
            .windows(3)
            .any(|w| w[0] == "--bind" && w[1] == "/home/user/proj" && w[2] == "/home/user/proj");
        assert!(found);
    }

    #[test]
    fn shadow_volumes_use_ro_bind() {
        let mut cfg = make_cfg(Harness::Pi);
        cfg.shadow_volumes = vec![("/tmp/empty".to_string(), "/project/.env".to_string())];
        let cmd = build_command(&cfg).unwrap();
        let found = cmd
            .windows(3)
            .any(|w| w[0] == "--ro-bind" && w[1] == "/tmp/empty" && w[2] == "/project/.env");
        assert!(found, "shadow volume should appear as --ro-bind");
    }

    #[test]
    fn env_vars_use_setenv() {
        let mut cfg = make_cfg(Harness::Pi);
        cfg.env_vars = vec![("MY_VAR".to_string(), "hello".to_string())];
        let cmd = build_command(&cfg).unwrap();
        let found = cmd
            .windows(3)
            .any(|w| w[0] == "--setenv" && w[1] == "MY_VAR" && w[2] == "hello");
        assert!(found);
    }

    #[test]
    fn no_browser_skips_agent_browser_mount() {
        let cfg = make_cfg(Harness::Pi); // no_browser = true
        let cmd = build_command(&cfg).unwrap();
        let joined = cmd.join(" ");
        assert!(!joined.contains(".agent-browser"));
    }

    #[test]
    fn with_browser_includes_agent_browser_mount() {
        let base = tempfile::tempdir().unwrap();
        let home = base.path().to_str().unwrap().to_string();
        let prev = std::env::var("HOME").ok();
        std::env::set_var("HOME", &home);

        let mut cfg = make_cfg(Harness::Pi);
        cfg.no_browser = false;
        let result = build_command(&cfg);

        match prev {
            Some(v) => std::env::set_var("HOME", v),
            None => std::env::remove_var("HOME"),
        }

        let cmd = result.unwrap();
        let browser_dir = format!("{home}/.agent-browser");
        let found = cmd
            .windows(3)
            .any(|w| w[0] == "--bind" && w[1] == browser_dir && w[2] == browser_dir);
        assert!(found, ".agent-browser should be mounted when browser is enabled");
    }
}
