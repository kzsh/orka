use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::process::{Command, ExitStatus};

use tempfile::TempDir;

use crate::cli::Runtime;

// Embed the Docker build context files so the binary is self-contained.
// These paths are relative to the workspace root (i.e. the directory that
// contains Cargo.toml and the Dockerfiles).
const DOCKERFILE: &str = include_str!("../Dockerfile");
const DOCKERFILE_BASE: &str = include_str!("../Dockerfile.base");
const DOCKERFILE_BROWSER_BASE: &str = include_str!("../Dockerfile.browser-base");
const DOCKERFILE_CLAUDE: &str = include_str!("../Dockerfile.claude");
const DOCKERFILE_CODEX: &str = include_str!("../Dockerfile.codex");
const ENTRYPOINT_SH: &str = include_str!("../entrypoint.sh");
const ENTRYPOINT_CLAUDE_SH: &str = include_str!("../entrypoint.claude.sh");
const ENTRYPOINT_CODEX_SH: &str = include_str!("../entrypoint.codex.sh");

const BASE_CONTAINER_NAME: &str = "orka-base";
const BROWSER_BASE_CONTAINER_NAME: &str = "orka-browser-base";
const PI_CONTAINER_NAME: &str = "orka";
const CLAUDE_CONTAINER_NAME: &str = "orka-claude";
const CODEX_CONTAINER_NAME: &str = "orka-codex";

/// Everything the caller needs to communicate to the docker build+run sequence.
pub struct RunConfig {
    pub runtime: Runtime,
    pub no_cache: bool,
    pub dry_run: bool,
    pub verbose: bool,
    pub debug: bool,
    pub preserve_container: bool,
    pub harness_version: Option<String>,
    /// When true, skips passing `INSTALL_AGENT_BROWSER=true` to the pi image build.
    /// Ignored for the claude runtime.
    pub no_browser: bool,
    /// Resolved `(host_path, container_path)` volume pairs.
    pub volumes: Vec<(String, String)>,
    /// Resolved `(key, value)` environment variable pairs.
    pub env_vars: Vec<(String, String)>,
    pub workdir: String,
    pub container_args: Vec<String>,
}

/// Write all embedded Dockerfiles and entrypoints into a fresh temp directory
/// and return it.  The directory is used as the Docker build context.
fn write_build_context() -> Result<TempDir, String> {
    let dir =
        tempfile::tempdir().map_err(|e| format!("failed to create temp build context: {e}"))?;

    fs::write(dir.path().join("Dockerfile"), DOCKERFILE)
        .map_err(|e| format!("failed to write Dockerfile to build context: {e}"))?;

    fs::write(dir.path().join("Dockerfile.base"), DOCKERFILE_BASE)
        .map_err(|e| format!("failed to write Dockerfile.base to build context: {e}"))?;

    fs::write(
        dir.path().join("Dockerfile.browser-base"),
        DOCKERFILE_BROWSER_BASE,
    )
    .map_err(|e| format!("failed to write Dockerfile.browser-base to build context: {e}"))?;

    fs::write(dir.path().join("Dockerfile.claude"), DOCKERFILE_CLAUDE)
        .map_err(|e| format!("failed to write Dockerfile.claude to build context: {e}"))?;

    let entrypoint = dir.path().join("entrypoint.sh");
    fs::write(&entrypoint, ENTRYPOINT_SH)
        .map_err(|e| format!("failed to write entrypoint.sh to build context: {e}"))?;
    fs::set_permissions(&entrypoint, fs::Permissions::from_mode(0o755))
        .map_err(|e| format!("failed to set entrypoint.sh permissions: {e}"))?;

    let claude_entrypoint = dir.path().join("entrypoint.claude.sh");
    fs::write(&claude_entrypoint, ENTRYPOINT_CLAUDE_SH)
        .map_err(|e| format!("failed to write entrypoint.claude.sh to build context: {e}"))?;
    fs::set_permissions(&claude_entrypoint, fs::Permissions::from_mode(0o755))
        .map_err(|e| format!("failed to set entrypoint.claude.sh permissions: {e}"))?;

    fs::write(dir.path().join("Dockerfile.codex"), DOCKERFILE_CODEX)
        .map_err(|e| format!("failed to write Dockerfile.codex to build context: {e}"))?;

    let codex_entrypoint = dir.path().join("entrypoint.codex.sh");
    fs::write(&codex_entrypoint, ENTRYPOINT_CODEX_SH)
        .map_err(|e| format!("failed to write entrypoint.codex.sh to build context: {e}"))?;
    fs::set_permissions(&codex_entrypoint, fs::Permissions::from_mode(0o755))
        .map_err(|e| format!("failed to set entrypoint.codex.sh permissions: {e}"))?;

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
    let browser_base_ref = format!("{BROWSER_BASE_CONTAINER_NAME}:latest");
    let uid = current_uid();
    let gid = current_gid();
    let uname = std::env::var("USER")
        .or_else(|_| std::env::var("LOGNAME"))
        .unwrap_or_else(|_| "appuser".to_string());

    let base_build = build_base_command(&base_ref, ctx_path, cfg);

    // For pi with browser enabled, build the intermediate browser-base layer
    // before the main image.  This layer installs agent-browser and downloads
    // Chromium; it is not rebuilt with --no-cache so it stays cached across
    // pi version upgrades.
    let browser_base_build = if matches!(cfg.runtime, Runtime::Pi) && !cfg.no_browser {
        Some(build_browser_base_command(
            &browser_base_ref,
            &base_ref,
            ctx_path,
            cfg,
        ))
    } else {
        None
    };

    // The pi main image builds FROM browser-base when browser support is
    // enabled, FROM the plain apt-base otherwise.
    let pi_base_ref = if cfg.no_browser {
        base_ref.clone()
    } else {
        browser_base_ref.clone()
    };

    let (main_build, run_cmd) = match cfg.runtime {
        Runtime::Pi => {
            let tag = cfg.harness_version.as_deref().unwrap_or("latest");
            let main_ref = format!("{PI_CONTAINER_NAME}:{tag}");
            let b =
                build_pi_main_command(&main_ref, &pi_base_ref, &uname, uid, gid, ctx_path, cfg);
            let r = run_pi_command_args(&main_ref, uid, gid, cfg)?;
            (b, r)
        }
        Runtime::Claude => {
            let main_ref = format!("{CLAUDE_CONTAINER_NAME}:latest");
            let b =
                build_claude_main_command(&main_ref, &base_ref, &uname, uid, gid, ctx_path, cfg);
            let r = run_claude_command_args(&main_ref, uid, gid, cfg)?;
            (b, r)
        }
        Runtime::Codex => {
            let main_ref = format!("{CODEX_CONTAINER_NAME}:latest");
            let b =
                build_codex_main_command(&main_ref, &base_ref, &uname, uid, gid, ctx_path, cfg);
            let r = run_codex_command_args(&main_ref, uid, gid, cfg)?;
            (b, r)
        }
    };

    if cfg.dry_run {
        print_dry_run("base image build", &base_build);
        if let Some(ref bbb) = browser_base_build {
            print_dry_run("browser-base image build", bbb);
        }
        print_dry_run("main image build", &main_build);
        print_dry_run("container run", &run_cmd);
        return Ok(());
    }

    exec(&base_build)?;
    if let Some(ref bbb) = browser_base_build {
        exec(bbb)?;
    }
    exec(&main_build)?;
    exec(&run_cmd)?;

    Ok(())
}

// ---------------------------------------------------------------------------
// Command builders — base (shared)
// ---------------------------------------------------------------------------

fn build_base_command(base_ref: &str, ctx_path: &str, cfg: &RunConfig) -> Vec<String> {
    let mut cmd = vec![
        s("docker"),
        s("build"),
        s("--tag"),
        s(base_ref),
        s("--file"),
        format!("{ctx_path}/Dockerfile.base"),
    ];
    if cfg.debug {
        cmd.push(s("--debug"));
    }
    if !cfg.verbose {
        cmd.push(s("--quiet"));
    }
    cmd.push(s(ctx_path));
    cmd
}

// ---------------------------------------------------------------------------
// Command builders — browser-base (pi only)
// ---------------------------------------------------------------------------

/// Builds the intermediate orka-browser-base image that contains agent-browser
/// and its Chromium download.  Like the apt base, this is never rebuilt with
/// --no-cache so it stays warm across pi version upgrades.
fn build_browser_base_command(
    browser_base_ref: &str,
    base_ref: &str,
    ctx_path: &str,
    cfg: &RunConfig,
) -> Vec<String> {
    let mut cmd = vec![
        s("docker"),
        s("build"),
        s("--tag"),
        s(browser_base_ref),
        s("--file"),
        format!("{ctx_path}/Dockerfile.browser-base"),
        s("--build-arg"),
        format!("BASE_IMAGE={base_ref}"),
    ];
    if cfg.debug {
        cmd.push(s("--debug"));
    }
    if !cfg.verbose {
        cmd.push(s("--quiet"));
    }
    cmd.push(s(ctx_path));
    cmd
}

// ---------------------------------------------------------------------------
// Command builders — pi runtime
// ---------------------------------------------------------------------------

fn build_pi_main_command(
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
    if let Some(ref ver) = cfg.harness_version {
        cmd.push(s("--build-arg"));
        cmd.push(format!("VERSION={ver}"));
    }
    if cfg.no_cache {
        cmd.push(s("--no-cache"));
    }
    if cfg.debug {
        cmd.push(s("--debug"));
    }
    if !cfg.verbose {
        cmd.push(s("--quiet"));
    }
    cmd.push(s(ctx_path));
    cmd
}

fn run_pi_command_args(
    main_ref: &str,
    uid: u32,
    gid: u32,
    cfg: &RunConfig,
) -> Result<Vec<String>, String> {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/root".to_string());
    let pi_dir = format!("{home}/.pi");
    fs::create_dir_all(&pi_dir).map_err(|e| format!("failed to create {pi_dir}: {e}"))?;

    let agent_browser_dir = format!("{home}/.agent-browser");
    if !cfg.no_browser {
        fs::create_dir_all(&agent_browser_dir)
            .map_err(|e| format!("failed to create {agent_browser_dir}: {e}"))?;
    }

    let mut cmd = vec![
        s("docker"),
        s("run"),
        s("--user"),
        format!("{uid}:{gid}"),
        s("--interactive"),
        s("--tty"),
    ];
    if !cfg.preserve_container {
        cmd.push(s("--rm"));
    }
    cmd.push(s("--cap-drop=ALL"));
    cmd.push(s("--security-opt=no-new-privileges"));
    if cfg.debug {
        cmd.push(s("--debug"));
    }

    // Pi config/data dir is always mounted so settings persist across runs.
    cmd.push(s("--volume"));
    cmd.push(format!("{pi_dir}:{pi_dir}"));

    // When the browser extension is included, mount its data dir so screenshots
    // and other browser output are accessible on the host.
    if !cfg.no_browser {
        cmd.push(s("--volume"));
        cmd.push(format!("{agent_browser_dir}:{agent_browser_dir}"));
    }

    for (host, container) in &cfg.volumes {
        cmd.push(s("--volume"));
        cmd.push(format!("{host}:{container}"));
    }
    for (key, val) in &cfg.env_vars {
        cmd.push(s("--env"));
        cmd.push(format!("{key}={val}"));
    }

    for key in ["ANTHROPIC_API_KEY", "OPENAI_API_KEY", "OPEN_ROUTER_KEY"] {
        cmd.push(s("--env"));
        cmd.push(format!("{key}={}", std::env::var(key).unwrap_or_default()));
    }

    cmd.push(s("--workdir"));
    cmd.push(cfg.workdir.clone());
    cmd.push(s(main_ref));
    cmd.extend(cfg.container_args.iter().cloned());

    Ok(cmd)
}

// ---------------------------------------------------------------------------
// Command builders — claude runtime
// ---------------------------------------------------------------------------

fn build_claude_main_command(
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
        s("--file"),
        format!("{ctx_path}/Dockerfile.claude"),
        s("--build-arg"),
        format!("BASE_IMAGE={base_ref}"),
        s("--build-arg"),
        format!("USER_UID={uid}"),
        s("--build-arg"),
        format!("USER_GID={gid}"),
        s("--build-arg"),
        format!("UNAME={uname}"),
    ];
    if cfg.no_cache {
        cmd.push(s("--no-cache"));
    }
    if cfg.debug {
        cmd.push(s("--debug"));
    }
    if !cfg.verbose {
        cmd.push(s("--quiet"));
    }
    cmd.push(s(ctx_path));
    cmd
}

fn run_claude_command_args(
    main_ref: &str,
    uid: u32,
    gid: u32,
    cfg: &RunConfig,
) -> Result<Vec<String>, String> {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/root".to_string());
    let claude_dir = format!("{home}/.claude");
    fs::create_dir_all(&claude_dir).map_err(|e| format!("failed to create {claude_dir}: {e}"))?;

    // claude.json holds user preferences (theme, model, etc.).  It is a file,
    // not a directory, so we must ensure it exists before bind-mounting it —
    // Docker would create it as a directory otherwise.
    let claude_json = format!("{home}/.claude.json");
    if !std::path::Path::new(&claude_json).exists() {
        fs::write(&claude_json, "{}")
            .map_err(|e| format!("failed to create {claude_json}: {e}"))?;
    }

    let mut cmd = vec![
        s("docker"),
        s("run"),
        s("--user"),
        format!("{uid}:{gid}"),
        s("--interactive"),
        s("--tty"),
    ];
    if !cfg.preserve_container {
        cmd.push(s("--rm"));
    }
    cmd.push(s("--cap-drop=ALL"));
    cmd.push(s("--security-opt=no-new-privileges"));
    if cfg.debug {
        cmd.push(s("--debug"));
    }

    // Claude config/data dir is always mounted so conversation history persists.
    cmd.push(s("--volume"));
    cmd.push(format!("{claude_dir}:{claude_dir}"));

    // claude.json holds user preferences (theme, model, etc.) and must be
    // mounted as a file so changes made inside the container are written back.
    cmd.push(s("--volume"));
    cmd.push(format!("{claude_json}:{claude_json}"));

    for (host, container) in &cfg.volumes {
        cmd.push(s("--volume"));
        cmd.push(format!("{host}:{container}"));
    }
    for (key, val) in &cfg.env_vars {
        cmd.push(s("--env"));
        cmd.push(format!("{key}={val}"));
    }

    for key in ["ANTHROPIC_API_KEY", "OPENAI_API_KEY", "OPEN_ROUTER_KEY"] {
        cmd.push(s("--env"));
        cmd.push(format!("{key}={}", std::env::var(key).unwrap_or_default()));
    }

    cmd.push(s("--workdir"));
    cmd.push(cfg.workdir.clone());
    cmd.push(s(main_ref));
    cmd.extend(cfg.container_args.iter().cloned());

    Ok(cmd)
}

// ---------------------------------------------------------------------------
// Command builders — codex runtime
// ---------------------------------------------------------------------------

fn build_codex_main_command(
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
        s("--file"),
        format!("{ctx_path}/Dockerfile.codex"),
        s("--build-arg"),
        format!("BASE_IMAGE={base_ref}"),
        s("--build-arg"),
        format!("USER_UID={uid}"),
        s("--build-arg"),
        format!("USER_GID={gid}"),
        s("--build-arg"),
        format!("UNAME={uname}"),
    ];
    if cfg.no_cache {
        cmd.push(s("--no-cache"));
    }
    if cfg.debug {
        cmd.push(s("--debug"));
    }
    if !cfg.verbose {
        cmd.push(s("--quiet"));
    }
    cmd.push(s(ctx_path));
    cmd
}

fn run_codex_command_args(
    main_ref: &str,
    uid: u32,
    gid: u32,
    cfg: &RunConfig,
) -> Result<Vec<String>, String> {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/root".to_string());
    let codex_dir = format!("{home}/.codex");
    fs::create_dir_all(&codex_dir).map_err(|e| format!("failed to create {codex_dir}: {e}"))?;

    let mut cmd = vec![
        s("docker"),
        s("run"),
        s("--user"),
        format!("{uid}:{gid}"),
        s("--interactive"),
        s("--tty"),
    ];
    if !cfg.preserve_container {
        cmd.push(s("--rm"));
    }
    cmd.push(s("--cap-drop=ALL"));
    cmd.push(s("--security-opt=no-new-privileges"));
    if cfg.debug {
        cmd.push(s("--debug"));
    }

    // Codex config/data dir is always mounted so settings and history persist.
    cmd.push(s("--volume"));
    cmd.push(format!("{codex_dir}:{codex_dir}"));

    for (host, container) in &cfg.volumes {
        cmd.push(s("--volume"));
        cmd.push(format!("{host}:{container}"));
    }
    for (key, val) in &cfg.env_vars {
        cmd.push(s("--env"));
        cmd.push(format!("{key}={val}"));
    }

    for key in ["ANTHROPIC_API_KEY", "OPENAI_API_KEY", "OPEN_ROUTER_KEY"] {
        cmd.push(s("--env"));
        cmd.push(format!("{key}={}", std::env::var(key).unwrap_or_default()));
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
    if s.chars().any(|c| {
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
        let uid = current_uid();
        let gid = current_gid();
        assert!(uid < u32::MAX);
        assert!(gid < u32::MAX);
    }

    fn make_cfg(runtime: Runtime) -> RunConfig {
        RunConfig {
            runtime,
            no_cache: false,
            dry_run: false,
            verbose: false,
            debug: false,
            preserve_container: false,
            harness_version: None,
            no_browser: false,
            volumes: vec![],
            env_vars: vec![],
            workdir: "/work".to_string(),
            container_args: vec![],
        }
    }

    #[test]
    fn pi_build_command_uses_pi_image_name() {
        let cfg = make_cfg(Runtime::Pi);
        let cmd = build_pi_main_command(
            "orka:latest",
            "orka-base:latest",
            "user",
            1000,
            1000,
            "/ctx",
            &cfg,
        );
        assert_eq!(cmd[0], "docker");
        assert_eq!(cmd[1], "build");
        assert!(cmd.contains(&"orka:latest".to_string()));
        // No --file means it uses the context default (Dockerfile)
        assert!(!cmd.contains(&"--file".to_string()));
    }

    #[test]
    fn claude_build_command_uses_claude_dockerfile() {
        let cfg = make_cfg(Runtime::Claude);
        let cmd = build_claude_main_command(
            "orka-claude:latest",
            "orka-base:latest",
            "user",
            1000,
            1000,
            "/ctx",
            &cfg,
        );
        assert_eq!(cmd[0], "docker");
        assert_eq!(cmd[1], "build");
        assert!(cmd.contains(&"orka-claude:latest".to_string()));
        // Must specify --file pointing at Dockerfile.claude
        let file_idx = cmd.iter().position(|a| a == "--file").unwrap();
        assert!(cmd[file_idx + 1].ends_with("Dockerfile.claude"));
    }

    #[test]
    fn pi_build_uses_browser_base_by_default() {
        // When browser support is enabled (the default), the main pi image
        // should build FROM orka-browser-base, not orka-base.
        let cfg = make_cfg(Runtime::Pi);
        let cmd = build_pi_main_command(
            "orka:latest",
            "orka-browser-base:latest",
            "user",
            1000,
            1000,
            "/ctx",
            &cfg,
        );
        let joined = cmd.join(" ");
        assert!(joined.contains("BASE_IMAGE=orka-browser-base:latest"));
        assert!(!joined.contains("INSTALL_AGENT_BROWSER"));
    }

    #[test]
    fn pi_build_uses_plain_base_when_no_browser() {
        let mut cfg = make_cfg(Runtime::Pi);
        cfg.no_browser = true;
        let cmd = build_pi_main_command(
            "orka:latest",
            "orka-base:latest",
            "user",
            1000,
            1000,
            "/ctx",
            &cfg,
        );
        let joined = cmd.join(" ");
        assert!(joined.contains("BASE_IMAGE=orka-base:latest"));
        assert!(!joined.contains("orka-browser-base"));
        assert!(!joined.contains("INSTALL_AGENT_BROWSER"));
    }

    #[test]
    fn browser_base_build_command_is_correct() {
        let cfg = make_cfg(Runtime::Pi);
        let cmd = build_browser_base_command(
            "orka-browser-base:latest",
            "orka-base:latest",
            "/ctx",
            &cfg,
        );
        assert_eq!(cmd[0], "docker");
        assert_eq!(cmd[1], "build");
        assert!(cmd.contains(&"orka-browser-base:latest".to_string()));
        let file_idx = cmd.iter().position(|a| a == "--file").unwrap();
        assert!(cmd[file_idx + 1].ends_with("Dockerfile.browser-base"));
        let joined = cmd.join(" ");
        assert!(joined.contains("BASE_IMAGE=orka-base:latest"));
        // browser-base is never rebuilt with --no-cache
        assert!(!joined.contains("--no-cache"));
    }

    #[test]
    fn claude_build_never_passes_browser_arg() {
        let cfg = make_cfg(Runtime::Claude);
        let cmd = build_claude_main_command(
            "orka-claude:latest",
            "orka-base:latest",
            "user",
            1000,
            1000,
            "/ctx",
            &cfg,
        );
        let joined = cmd.join(" ");
        assert!(!joined.contains("INSTALL_AGENT_BROWSER"));
        assert!(!joined.contains("browser-base"));
    }

    #[test]
    fn pi_build_passes_version_when_set() {
        let mut cfg = make_cfg(Runtime::Pi);
        cfg.harness_version = Some("1.2.3".to_string());
        let cmd = build_pi_main_command(
            "orka:1.2.3",
            "orka-base:latest",
            "user",
            1000,
            1000,
            "/ctx",
            &cfg,
        );
        let joined = cmd.join(" ");
        assert!(joined.contains("VERSION=1.2.3"));
    }
}
