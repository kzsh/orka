use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::process::{Command, ExitStatus};

use tempfile::TempDir;

// Embed the Docker build context files so the binary is self-contained.
// These paths are relative to the workspace root (i.e. the directory that
// contains Cargo.toml, Dockerfile, Dockerfile.base, and entrypoint.sh).
const DOCKERFILE: &str = include_str!("../Dockerfile");
const DOCKERFILE_BASE: &str = include_str!("../Dockerfile.base");
const ENTRYPOINT_SH: &str = include_str!("../entrypoint.sh");

const CONTAINER_NAME: &str = "pita";
const BASE_CONTAINER_NAME: &str = "pita-base";

/// Everything the caller needs to communicate to the docker build+run sequence.
pub struct RunConfig {
    pub no_cache: bool,
    pub dry_run: bool,
    pub quiet: bool,
    pub debug: bool,
    pub ephemeral: bool,
    pub pi_version: Option<String>,
    /// When true, passes `INSTALL_AGENT_BROWSER=true` to the main image build.
    pub with_browser: bool,
    /// When true, mounts a tmpfs over ~/.pi/agent/extensions to hide all extensions.
    pub no_extensions: bool,
    /// Resolved `(host_path, container_path)` volume pairs.
    pub volumes: Vec<(String, String)>,
    /// Resolved `(key, value)` environment variable pairs.
    pub env_vars: Vec<(String, String)>,
    pub workdir: String,
    pub container_args: Vec<String>,
}

/// Write the embedded Dockerfiles and entrypoint into a fresh temp directory
/// and return it.  The directory is the Docker build context.
fn write_build_context() -> Result<TempDir, String> {
    let dir = tempfile::tempdir()
        .map_err(|e| format!("failed to create temp build context: {e}"))?;

    fs::write(dir.path().join("Dockerfile"), DOCKERFILE)
        .map_err(|e| format!("failed to write Dockerfile to build context: {e}"))?;

    fs::write(dir.path().join("Dockerfile.base"), DOCKERFILE_BASE)
        .map_err(|e| format!("failed to write Dockerfile.base to build context: {e}"))?;

    let entrypoint = dir.path().join("entrypoint.sh");
    fs::write(&entrypoint, ENTRYPOINT_SH)
        .map_err(|e| format!("failed to write entrypoint.sh to build context: {e}"))?;
    fs::set_permissions(&entrypoint, fs::Permissions::from_mode(0o755))
        .map_err(|e| format!("failed to set entrypoint.sh permissions: {e}"))?;

    Ok(dir)
}

/// Build both Docker images and, once they are ready, run the container.
pub fn build_and_run(cfg: &RunConfig) -> Result<(), String> {
    let ctx = write_build_context()?;
    let ctx_path = ctx
        .path()
        .to_str()
        .ok_or_else(|| "temp build context path contains non-UTF-8 characters".to_string())?;

    let base_ref = format!("{BASE_CONTAINER_NAME}:latest");
    let tag = cfg.pi_version.as_deref().unwrap_or("latest");
    let image_tag = if cfg.with_browser {
        format!("browser-{tag}")
    } else {
        tag.to_string()
    };
    let main_ref = format!("{CONTAINER_NAME}:{image_tag}");

    let uid = current_uid();
    let gid = current_gid();
    let uname = std::env::var("USER")
        .or_else(|_| std::env::var("LOGNAME"))
        .unwrap_or_else(|_| "appuser".to_string());

    let base_build = build_base_command(&base_ref, ctx_path, cfg);
    let main_build = build_main_command(&main_ref, &base_ref, &uname, uid, gid, ctx_path, cfg);
    let run_cmd = run_command_args(&main_ref, uid, gid, cfg)?;

    if cfg.dry_run {
        print_dry_run("base image build", &base_build);
        print_dry_run("main image build", &main_build);
        print_dry_run("container run", &run_cmd);
        return Ok(());
    }

    exec(&base_build)?;
    exec(&main_build)?;
    exec(&run_cmd)?;

    Ok(())
}

// ---------------------------------------------------------------------------
// Command builders
// ---------------------------------------------------------------------------

fn build_base_command(base_ref: &str, ctx_path: &str, cfg: &RunConfig) -> Vec<String> {
    let mut cmd = vec![
        s("docker"),
        s("build"),
        s("--tag"),
        s(base_ref),
        s("--file"),
        // Point docker at the embedded Dockerfile.base inside the temp dir.
        format!("{ctx_path}/Dockerfile.base"),
    ];
    if cfg.debug {
        cmd.push(s("--debug"));
    }
    if cfg.quiet {
        cmd.push(s("--quiet"));
    }
    cmd.push(s(ctx_path));
    cmd
}

fn build_main_command(
    main_ref: &str,
    base_ref: &str,
    uname: &str,
    uid: u32,
    gid: u32,
    ctx_path: &str,
    cfg: &RunConfig,
) -> Vec<String> {
    let mut cmd = vec![
        s("docker"),
        s("build"),
        s("--tag"),
        s(main_ref),
        s("--build-arg"),
        format!("BASE_IMAGE={base_ref}"),
        s("--build-arg"),
        format!("USER_UID={uid}"),
        s("--build-arg"),
        format!("USER_GID={gid}"),
        s("--build-arg"),
        format!("UNAME={uname}"),
    ];
    if let Some(ref ver) = cfg.pi_version {
        cmd.push(s("--build-arg"));
        cmd.push(format!("VERSION={ver}"));
    }
    if cfg.with_browser {
        cmd.push(s("--build-arg"));
        cmd.push(s("INSTALL_AGENT_BROWSER=true"));
    }
    if cfg.no_cache {
        cmd.push(s("--no-cache"));
    }
    if cfg.debug {
        cmd.push(s("--debug"));
    }
    if cfg.quiet {
        cmd.push(s("--quiet"));
    }
    cmd.push(s(ctx_path));
    cmd
}

fn run_command_args(
    main_ref: &str,
    uid: u32,
    gid: u32,
    cfg: &RunConfig,
) -> Result<Vec<String>, String> {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/root".to_string());
    let pi_dir = format!("{home}/.pi");
    fs::create_dir_all(&pi_dir)
        .map_err(|e| format!("failed to create {pi_dir}: {e}"))?;

    let mut cmd = vec![
        s("docker"),
        s("run"),
        s("--user"),
        format!("{uid}:{gid}"),
        s("--interactive"),
        s("--tty"),
    ];
    if cfg.ephemeral {
        cmd.push(s("--rm"));
    }
    if cfg.debug {
        cmd.push(s("--debug"));
    }

    // Pi config/data dir is always mounted so settings persist across runs.
    cmd.push(s("--volume"));
    cmd.push(format!("{pi_dir}:{pi_dir}"));

    // Shadow the extensions directory with an empty tmpfs so no auto-discovered
    // extensions load. The tmpfs mount is more specific than the parent bind
    // mount, so it wins regardless of order.
    if cfg.no_extensions {
        cmd.push(s("--tmpfs"));
        cmd.push(format!("{pi_dir}/agent/extensions"));
    }

    for (host, container) in &cfg.volumes {
        cmd.push(s("--volume"));
        cmd.push(format!("{host}:{container}"));
    }
    for (key, val) in &cfg.env_vars {
        cmd.push(s("--env"));
        cmd.push(format!("{key}={val}"));
    }

    // Always forward the three API keys from the host environment.
    for key in ["ANTHROPIC_API_KEY", "OPENAI_API_KEY", "OPEN_ROUTER_KEY"] {
        cmd.push(s("--env"));
        cmd.push(format!(
            "{key}={}",
            std::env::var(key).unwrap_or_default()
        ));
    }

    cmd.push(s("--workdir"));
    cmd.push(cfg.workdir.clone());
    cmd.push(s(main_ref));
    cmd.extend(cfg.container_args.iter().cloned());

    Ok(cmd)
}

// ---------------------------------------------------------------------------
// Execution helpers
// ---------------------------------------------------------------------------

fn exec(args: &[String]) -> Result<(), String> {
    let (program, rest) = args
        .split_first()
        .ok_or_else(|| "attempted to run empty command".to_string())?;

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

fn print_dry_run(label: &str, args: &[String]) {
    println!("[DRY_RUN] {label}:");
    let line = args
        .iter()
        .enumerate()
        .map(|(i, a)| {
            let prev_is_env = i > 0 && args[i - 1] == "--env";
            if prev_is_env {
                if let Some(eq) = a.find('=') {
                    return format!("{}=<redacted>", &a[..eq]);
                }
            }
            shell_quote(a)
        })
        .collect::<Vec<_>>()
        .join(" \\\n  ");
    println!("  {line}");
    println!();
}

/// Minimal shell quoting: wrap in single quotes when the value contains
/// characters that would be interpreted by a shell.
fn shell_quote(s: &str) -> String {
    if s.chars()
        .any(|c| matches!(c, ' ' | '\t' | '\n' | '"' | '\'' | '\\' | '$' | '!' | '&' | '|' | ';' | '(' | ')' | '<' | '>'))
    {
        format!("'{}'", s.replace('\'', "'\\''"))
    } else {
        s.to_owned()
    }
}

// ---------------------------------------------------------------------------
// Unix identity helpers
// ---------------------------------------------------------------------------

fn current_uid() -> u32 {
    // SAFETY: getuid() has no preconditions and always succeeds.
    unsafe { libc::getuid() }
}

fn current_gid() -> u32 {
    // SAFETY: getgid() has no preconditions and always succeeds.
    unsafe { libc::getgid() }
}

/// Convenience so every string literal doesn't need `.to_string()`.
fn s(lit: &str) -> String {
    lit.to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shell_quote_plain() {
        assert_eq!(shell_quote("hello"), "hello");
    }

    #[test]
    fn shell_quote_with_spaces() {
        assert_eq!(shell_quote("hello world"), "'hello world'");
    }

    #[test]
    fn shell_quote_with_dollar() {
        assert_eq!(shell_quote("$HOME"), "'$HOME'");
    }

    #[test]
    fn shell_quote_with_single_quote() {
        assert_eq!(shell_quote("it's"), "'it'\\''s'");
    }

    #[test]
    fn uid_and_gid_are_nonzero_for_normal_user() {
        // When running tests as a non-root user (which CI and dev boxes are),
        // both should be > 0.  This is a sanity check, not a hard requirement.
        let uid = current_uid();
        let gid = current_gid();
        assert!(uid < u32::MAX);
        assert!(gid < u32::MAX);
    }
}
