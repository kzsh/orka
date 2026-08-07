use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::process::{Command, ExitStatus};

use tempfile::TempDir;

use crate::cli::{Backend, Harness};

// Embed the Docker build context files so the binary is self-contained.
// These paths are relative to the workspace root (i.e. the directory that
// contains Cargo.toml and the Dockerfiles).
const DOCKERFILE: &str = include_str!("../Dockerfile");
const DOCKERFILE_BASE: &str = include_str!("../Dockerfile.base");
const DOCKERFILE_CLAUDE: &str = include_str!("../Dockerfile.claude");
const DOCKERFILE_CODEX: &str = include_str!("../Dockerfile.codex");
const ENTRYPOINT_SH: &str = include_str!("../entrypoint.sh");
const ENTRYPOINT_CLAUDE_SH: &str = include_str!("../entrypoint.claude.sh");
const ENTRYPOINT_CODEX_SH: &str = include_str!("../entrypoint.codex.sh");

const BASE_CONTAINER_NAME: &str = "orka-base";
const PI_CONTAINER_NAME: &str = "orka";
const CLAUDE_CONTAINER_NAME: &str = "orka-claude";
const CODEX_CONTAINER_NAME: &str = "orka-codex";

/// Everything the caller needs to communicate to the build+run sequence.
pub struct RunConfig {
    /// Binary name of the container engine (e.g. "docker", "podman").
    pub engine_binary: String,
    pub backend: Backend,
    pub harness: Harness,
    pub no_cache: bool,
    pub dry_run: bool,
    pub verbose: bool,
    pub quiet: bool,
    pub preserve_container: bool,
    pub harness_version: Option<String>,
    /// Resolved `(host_path, container_path)` volume pairs.
    pub volumes: Vec<(String, String)>,
    /// Read-only shadow volumes: each is mounted `:ro` over a sensitive path
    /// inside the container so the original file is hidden and writes are refused.
    pub shadow_volumes: Vec<(String, String)>,
    /// Resolved `(key, value)` environment variable pairs.
    pub env_vars: Vec<(String, String)>,
    /// Names of the presets applied to this run, in the order they were applied.
    pub presets: Vec<String>,
    /// Explicit path to the agent binary.  Used only by the bwrap backend;
    /// ignored by all container-engine paths.
    pub harness_binary: Option<String>,
    pub workdir: String,
    pub container_args: Vec<String>,
}

/// Startup banner printed before the image build and container run.
///
/// `label` identifies the harness (and backend, for bwrap).  Presets are listed
/// only when at least one is active, including always-on ones from config.yaml.
pub fn banner(label: &str, presets: &[String]) -> String {
    const RULE: &str = "=====================";
    let mut out = format!("{RULE}\nOrka: {label}\n");
    if !presets.is_empty() {
        out.push_str(&format!("Presets: {}\n", presets.join(", ")));
    }
    out.push_str(RULE);
    out
}

/// Write all embedded Dockerfiles and entrypoints into a fresh temp directory
/// and return it.  The directory is used as the Docker build context.
///
/// When `~/.config/orka/Dockerfile.base` exists it is used in place of the
/// embedded base.
fn write_build_context() -> Result<TempDir, String> {
    let dir =
        tempfile::tempdir().map_err(|e| format!("failed to create temp build context: {e}"))?;

    fs::write(dir.path().join("Dockerfile"), DOCKERFILE)
        .map_err(|e| format!("failed to write Dockerfile to build context: {e}"))?;

    let custom_base_path = crate::config::custom_dockerfile_base_path();
    let dockerfile_base_content = if custom_base_path.is_file() {
        fs::read_to_string(&custom_base_path)
            .map_err(|e| format!("failed to read {}: {e}", custom_base_path.display()))?
    } else {
        DOCKERFILE_BASE.to_string()
    };
    fs::write(dir.path().join("Dockerfile.base"), dockerfile_base_content)
        .map_err(|e| format!("failed to write Dockerfile.base to build context: {e}"))?;

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
///
/// The harness image is always rebuilt so that the baked-in UNAME/UID/GID
/// stay in sync with the host user running orka.  The base image (apt deps)
/// is only built when it is absent or when `--no-cache` is passed, because
/// its content changes rarely and rebuilding it is slow.
pub fn build_and_run(cfg: &RunConfig) -> Result<(), String> {
    let ctx;
    let ctx_path_owned;

    let base_ref = format!("{BASE_CONTAINER_NAME}:latest");
    let uid = current_uid();
    let gid = current_gid();
    let uname = std::env::var("USER")
        .or_else(|_| std::env::var("LOGNAME"))
        .unwrap_or_else(|_| "appuser".to_string());

    if cfg.backend.is_apple_container() && !cfg.dry_run {
        ensure_apple_container_running(&cfg.engine_binary)?;
    }

    let need_base = cfg.no_cache || !image_exists(&cfg.engine_binary, cfg.backend, &base_ref);

    let tag = match cfg.harness {
        Harness::Pi => cfg
            .harness_version
            .as_deref()
            .unwrap_or("latest")
            .to_string(),
        _ => "latest".to_string(),
    };
    let main_ref = match cfg.harness {
        Harness::Pi => format!("{PI_CONTAINER_NAME}:{tag}"),
        Harness::Claude => format!("{CLAUDE_CONTAINER_NAME}:latest"),
        Harness::Codex => format!("{CODEX_CONTAINER_NAME}:latest"),
    };
    let need_main = true;

    // Only write the build context when at least one image needs building.
    let base_build;
    let main_build;
    if need_base || need_main {
        ctx = write_build_context()?;
        ctx_path_owned = ctx
            .path()
            .to_str()
            .ok_or_else(|| "temp build context path contains non-UTF-8 characters".to_string())?
            .to_string();
        let ctx_path = &ctx_path_owned;

        base_build = if need_base {
            Some(build_base_command(&base_ref, ctx_path, cfg))
        } else {
            None
        };
        main_build = if need_main {
            let cmd = match cfg.harness {
                Harness::Pi => {
                    build_pi_main_command(&main_ref, &base_ref, &uname, uid, gid, ctx_path, cfg)
                }
                Harness::Claude => {
                    build_claude_main_command(&main_ref, &base_ref, &uname, uid, gid, ctx_path, cfg)
                }
                Harness::Codex => {
                    build_codex_main_command(&main_ref, &base_ref, &uname, uid, gid, ctx_path, cfg)
                }
            };
            Some(cmd)
        } else {
            None
        };
    } else {
        base_build = None;
        main_build = None;
    }

    let caps = EngineCapabilities::detect(&cfg.engine_binary, cfg.backend);

    let run_cmd = match cfg.harness {
        Harness::Pi => run_pi_command_args(&main_ref, uid, gid, caps, cfg)?,
        Harness::Claude => run_claude_command_args(&main_ref, uid, gid, caps, cfg)?,
        Harness::Codex => run_codex_command_args(&main_ref, uid, gid, caps, cfg)?,
    };

    if cfg.dry_run {
        if let Some(ref cmd) = base_build {
            print_dry_run("base image build", cmd);
        } else {
            println!("[DRY_RUN] base image build: skipped (image exists)");
        }
        if let Some(ref cmd) = main_build {
            print_dry_run("main image build", cmd);
        } else {
            println!("[DRY_RUN] main image build: skipped (image exists)");
        }
        print_dry_run("container run", &run_cmd);
        return Ok(());
    }

    if !cfg.verbose {
        let harness_label = match cfg.harness {
            Harness::Pi => "pi",
            Harness::Claude => "claude",
            Harness::Codex => "codex",
        };
        println!("{}", banner(harness_label, &cfg.presets));
    }

    if let Some(ref cmd) = base_build {
        exec(cmd)?;
    }
    if let Some(ref cmd) = main_build {
        exec(cmd)?;
    }
    exec(&run_cmd)?;

    Ok(())
}

// ---------------------------------------------------------------------------
// Command builders — base (shared)
// ---------------------------------------------------------------------------

fn build_base_command(base_ref: &str, ctx_path: &str, cfg: &RunConfig) -> Vec<String> {
    let mut cmd = vec![
        cfg.engine_binary.clone(),
        s("build"),
        s("--tag"),
        s(base_ref),
        s("--file"),
        format!("{ctx_path}/Dockerfile.base"),
    ];
    if cfg.quiet {
        cmd.push(s("--quiet"));
    }
    cmd.push(s(ctx_path));
    cmd
}

// ---------------------------------------------------------------------------
// Command builders — pi harness
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
        cfg.engine_binary.clone(),
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
    if cfg.quiet {
        cmd.push(s("--quiet"));
    }
    cmd.push(s(ctx_path));
    cmd
}

fn run_pi_command_args(
    main_ref: &str,
    uid: u32,
    gid: u32,
    caps: EngineCapabilities,
    cfg: &RunConfig,
) -> Result<Vec<String>, String> {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/root".to_string());
    let pi_dir = format!("{home}/.pi");
    fs::create_dir_all(&pi_dir).map_err(|e| format!("failed to create {pi_dir}: {e}"))?;

    let mut cmd = vec![
        cfg.engine_binary.clone(),
        s("run"),
        s("--interactive"),
        s("--tty"),
    ];
    if !cfg.preserve_container {
        cmd.push(s("--rm"));
    }
    push_isolation_args(&mut cmd, uid, gid, caps, cfg);

    // Pi config/data dir is always mounted so settings persist across runs.
    cmd.push(s("--volume"));
    cmd.push(format!("{pi_dir}:{pi_dir}"));

    for (host, container) in &cfg.volumes {
        cmd.push(s("--volume"));
        cmd.push(format!("{host}:{container}"));
    }
    for (source, container) in &cfg.shadow_volumes {
        cmd.push(s("--volume"));
        cmd.push(format!("{source}:{container}:ro"));
    }
    for (key, val) in &cfg.env_vars {
        cmd.push(s("--env"));
        cmd.push(format!("{key}={val}"));
    }

    for key in ["ANTHROPIC_API_KEY", "OPENAI_API_KEY", "OPEN_ROUTER_KEY"] {
        cmd.push(s("--env"));
        cmd.push(format!("{key}={}", std::env::var(key).unwrap_or_default()));
    }

    if cfg.verbose {
        cmd.push(s("--env"));
        cmd.push(s("VERBOSE=1"));
    }

    cmd.push(s("--workdir"));
    cmd.push(cfg.workdir.clone());
    cmd.push(s(main_ref));
    cmd.extend(cfg.container_args.iter().cloned());

    Ok(cmd)
}

// ---------------------------------------------------------------------------
// Command builders — claude harness
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
        cfg.engine_binary.clone(),
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
    if cfg.quiet {
        cmd.push(s("--quiet"));
    }
    cmd.push(s(ctx_path));
    cmd
}

fn run_claude_command_args(
    main_ref: &str,
    uid: u32,
    gid: u32,
    caps: EngineCapabilities,
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
        cfg.engine_binary.clone(),
        s("run"),
        s("--interactive"),
        s("--tty"),
    ];
    if !cfg.preserve_container {
        cmd.push(s("--rm"));
    }
    push_isolation_args(&mut cmd, uid, gid, caps, cfg);

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
    for (source, container) in &cfg.shadow_volumes {
        cmd.push(s("--volume"));
        cmd.push(format!("{source}:{container}:ro"));
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
// Command builders — codex harness
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
        cfg.engine_binary.clone(),
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
    if cfg.quiet {
        cmd.push(s("--quiet"));
    }
    cmd.push(s(ctx_path));
    cmd
}

fn run_codex_command_args(
    main_ref: &str,
    uid: u32,
    gid: u32,
    caps: EngineCapabilities,
    cfg: &RunConfig,
) -> Result<Vec<String>, String> {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/root".to_string());
    let codex_dir = format!("{home}/.codex");
    fs::create_dir_all(&codex_dir).map_err(|e| format!("failed to create {codex_dir}: {e}"))?;

    let mut cmd = vec![
        cfg.engine_binary.clone(),
        s("run"),
        s("--interactive"),
        s("--tty"),
    ];
    if !cfg.preserve_container {
        cmd.push(s("--rm"));
    }
    push_isolation_args(&mut cmd, uid, gid, caps, cfg);

    // Codex config/data dir is always mounted so settings and history persist.
    cmd.push(s("--volume"));
    cmd.push(format!("{codex_dir}:{codex_dir}"));

    for (host, container) in &cfg.volumes {
        cmd.push(s("--volume"));
        cmd.push(format!("{host}:{container}"));
    }
    for (source, container) in &cfg.shadow_volumes {
        cmd.push(s("--volume"));
        cmd.push(format!("{source}:{container}:ro"));
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

/// Sandbox flags whose availability varies between engines and, for Apple's
/// `container`, between versions of the same engine.  Unknown flags are a fatal
/// parse error there, so they are probed rather than assumed.
#[derive(Clone, Copy, Debug)]
struct EngineCapabilities {
    cap_drop: bool,
}

impl EngineCapabilities {
    /// Docker and Podman have supported `--cap-drop` for their entire history.
    /// Apple's `container` gained it in 0.12.0, so ask the binary itself.
    fn detect(engine: &str, backend: Backend) -> Self {
        if !backend.is_apple_container() {
            return Self { cap_drop: true };
        }
        Self {
            cap_drop: engine_run_help_mentions(engine, "--cap-drop"),
        }
    }
}

/// Returns true when `<engine> run --help` lists `flag`.  A missing binary or a
/// failed invocation reports false, which drops the flag: an over-permissive
/// container beats one that refuses to start.
fn engine_run_help_mentions(engine: &str, flag: &str) -> bool {
    Command::new(engine)
        .args(["run", "--help"])
        .output()
        .map(|out| {
            String::from_utf8_lossy(&out.stdout).contains(flag)
                || String::from_utf8_lossy(&out.stderr).contains(flag)
        })
        .unwrap_or(false)
}

/// Push the sandbox and user-identity flags shared by every harness run
/// command.
fn push_isolation_args(
    cmd: &mut Vec<String>,
    uid: u32,
    gid: u32,
    caps: EngineCapabilities,
    cfg: &RunConfig,
) {
    if caps.cap_drop {
        cmd.push(s("--cap-drop=ALL"));
    }

    // Apple's `container` has no --security-opt and rejects unknown flags.
    if !cfg.backend.is_apple_container() {
        cmd.push(s("--security-opt=no-new-privileges"));
    }

    if matches!(cfg.backend, Backend::Podman) {
        // --userns=keep-id maps the host user into the container at the same
        // UID without a name lookup.  Passing --user uid:gid on top would
        // make Podman reverse-resolve the numeric UID to a username, which
        // fails for LDAP/sssd users that are absent from /etc/passwd.
        cmd.push(s("--userns=keep-id"));
    } else {
        cmd.push(s("--user"));
        cmd.push(format!("{uid}:{gid}"));
    }
}

/// Returns true when the image `image_ref` is already present in the local
/// store of the given container engine.  A non-zero exit code (image not
/// found) is silently treated as `false`; actual execution errors (engine
/// binary missing, etc.) also return `false` so the caller falls back to a
/// normal build.
fn image_exists(engine: &str, backend: Backend, image_ref: &str) -> bool {
    let mut args = vec!["image", "inspect"];
    // Apple's `container image inspect` has no --format; it always emits JSON
    // and the exit status alone answers the question.
    if !backend.is_apple_container() {
        args.extend(["--format", "{{.Id}}"]);
    }
    args.push(image_ref);

    Command::new(engine)
        .args(args)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Apple's `container` CLI talks to a launchd-managed API server; every
/// subcommand fails with a low-level XPC error when it is not running.  Probe
/// it once so the user gets an actionable message instead.
fn ensure_apple_container_running(engine: &str) -> Result<(), String> {
    let status = Command::new(engine)
        .args(["system", "status"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map_err(|e| {
            format!(
                "failed to launch `{engine}`: {e}\n\
                 note: --engine container requires Apple's container CLI \
                 (https://github.com/apple/container) on macOS 26 or later"
            )
        })?;

    if status.success() {
        Ok(())
    } else {
        Err(format!(
            "`{engine} system status` reports the container services are not running\n\
             note: start them with `{engine} system start`"
        ))
    }
}

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

    #[test]
    fn banner_without_presets_omits_the_preset_line() {
        assert_eq!(
            banner("pi", &[]),
            "=====================\nOrka: pi\n====================="
        );
    }

    #[test]
    fn banner_lists_active_presets_in_order() {
        let presets = vec!["jira".to_string(), "rust".to_string()];
        assert_eq!(
            banner("pi (bwrap)", &presets),
            "=====================\nOrka: pi (bwrap)\nPresets: jira, rust\n====================="
        );
    }

    fn make_cfg(harness: Harness) -> RunConfig {
        RunConfig {
            engine_binary: "docker".to_string(),
            backend: Backend::Docker,
            harness,
            no_cache: false,
            dry_run: false,
            verbose: false,
            quiet: false,
            preserve_container: false,
            harness_version: None,
            volumes: vec![],
            shadow_volumes: vec![],
            env_vars: vec![],
            presets: vec![],
            harness_binary: None,
            workdir: "/work".to_string(),
            container_args: vec![],
        }
    }

    #[test]
    fn pi_build_command_uses_pi_image_name() {
        let cfg = make_cfg(Harness::Pi);
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
        assert!(!cmd.contains(&"--file".to_string()));
    }

    #[test]
    fn engine_binary_is_used_as_first_token() {
        let mut cfg = make_cfg(Harness::Pi);
        cfg.engine_binary = "podman".to_string();
        let cmd = build_pi_main_command(
            "orka:latest",
            "orka-base:latest",
            "user",
            1000,
            1000,
            "/ctx",
            &cfg,
        );
        assert_eq!(cmd[0], "podman");
    }

    #[test]
    fn claude_build_command_uses_claude_dockerfile() {
        let cfg = make_cfg(Harness::Claude);
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
    fn podman_run_adds_userns_keep_id() {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/root".to_string());
        let pi_dir = format!("{home}/.pi");
        std::fs::create_dir_all(&pi_dir).unwrap();

        let mut cfg = make_cfg(Harness::Pi);
        cfg.engine_binary = "podman".to_string();
        cfg.backend = Backend::Podman;
        let cmd = run_pi_command_args("orka:latest", 1000, 1000, ALL_CAPS, &cfg).unwrap();
        assert!(cmd.contains(&"--userns=keep-id".to_string()));
    }

    #[test]
    fn docker_run_omits_userns_keep_id() {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/root".to_string());
        let pi_dir = format!("{home}/.pi");
        std::fs::create_dir_all(&pi_dir).unwrap();

        let cfg = make_cfg(Harness::Pi);
        let cmd = run_pi_command_args("orka:latest", 1000, 1000, ALL_CAPS, &cfg).unwrap();
        assert!(!cmd.contains(&"--userns=keep-id".to_string()));
    }

    /// An engine that accepts every optional sandbox flag.
    const ALL_CAPS: EngineCapabilities = EngineCapabilities { cap_drop: true };

    /// Building a run command for `harness` under `backend`, with the
    /// per-harness home directories pre-created so the builders succeed.
    fn run_args_for(harness: Harness, backend: Backend) -> Vec<String> {
        run_args_with_caps(harness, backend, ALL_CAPS)
    }

    fn run_args_with_caps(
        harness: Harness,
        backend: Backend,
        caps: EngineCapabilities,
    ) -> Vec<String> {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/root".to_string());
        for sub in [".pi", ".claude", ".codex"] {
            std::fs::create_dir_all(format!("{home}/{sub}")).unwrap();
        }

        let mut cfg = make_cfg(harness);
        cfg.backend = backend;
        cfg.engine_binary = backend.binary().to_string();

        match harness {
            Harness::Pi => run_pi_command_args("orka:latest", 1000, 1000, caps, &cfg),
            Harness::Claude => run_claude_command_args("orka-claude:latest", 1000, 1000, caps, &cfg),
            Harness::Codex => run_codex_command_args("orka-codex:latest", 1000, 1000, caps, &cfg),
        }
        .unwrap()
    }

    /// Podman resolves a numeric `--user` back to a username, which fails for
    /// LDAP/sssd accounts missing from /etc/passwd.  `--userns=keep-id`
    /// already pins the container UID to the host UID, so `--user` must be
    /// omitted entirely under Podman.
    #[test]
    fn podman_run_omits_user_flag_for_every_harness() {
        for harness in [Harness::Pi, Harness::Claude, Harness::Codex] {
            let cmd = run_args_for(harness, Backend::Podman);
            assert!(
                !cmd.contains(&"--user".to_string()),
                "podman command must not pass --user: {cmd:?}"
            );
            assert!(
                cmd.contains(&"--userns=keep-id".to_string()),
                "podman command must pass --userns=keep-id: {cmd:?}"
            );
        }
    }

    #[test]
    fn docker_run_passes_user_flag_for_every_harness() {
        for harness in [Harness::Pi, Harness::Claude, Harness::Codex] {
            let cmd = run_args_for(harness, Backend::Docker);
            let idx = cmd
                .iter()
                .position(|a| a == "--user")
                .unwrap_or_else(|| panic!("docker command must pass --user: {cmd:?}"));
            assert_eq!(cmd[idx + 1], "1000:1000");
            assert!(!cmd.contains(&"--userns=keep-id".to_string()));
        }
    }

    /// The image reference must remain the final token before any trailing
    /// container arguments; reordering the flag block must not disturb it.
    #[test]
    fn image_ref_precedes_container_args() {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/root".to_string());
        std::fs::create_dir_all(format!("{home}/.pi")).unwrap();

        let mut cfg = make_cfg(Harness::Pi);
        cfg.backend = Backend::Podman;
        cfg.engine_binary = "podman".to_string();
        cfg.container_args = vec!["--help".to_string()];

        let cmd = run_pi_command_args("orka:latest", 1000, 1000, ALL_CAPS, &cfg).unwrap();
        let img = cmd.iter().position(|a| a == "orka:latest").unwrap();
        assert_eq!(cmd[img + 1], "--help");
        assert_eq!(cmd[img - 1], cfg.workdir);
    }

    #[test]
    fn image_exists_returns_false_for_nonexistent_image() {
        // Use a tag that cannot exist in any real registry to guarantee absence.
        assert!(!image_exists(
            "docker",
            Backend::Docker,
            "orka-test-does-not-exist:__never__"
        ));
    }

    /// `container` rejects unknown flags outright, and `--security-opt` is not
    /// part of its surface.  Every harness must omit it while still dropping
    /// capabilities, on the versions that accept `--cap-drop`, and pinning the
    /// user.
    #[test]
    fn apple_container_run_omits_security_opt_for_every_harness() {
        for harness in [Harness::Pi, Harness::Claude, Harness::Codex] {
            let cmd = run_args_for(harness, Backend::Container);
            assert_eq!(cmd[0], "container");
            assert!(
                !cmd.iter().any(|a| a.starts_with("--security-opt")),
                "container command must not pass --security-opt: {cmd:?}"
            );
            assert!(
                cmd.contains(&"--cap-drop=ALL".to_string()),
                "container command must drop capabilities: {cmd:?}"
            );
            assert!(!cmd.contains(&"--userns=keep-id".to_string()));
            let idx = cmd
                .iter()
                .position(|a| a == "--user")
                .unwrap_or_else(|| panic!("container command must pass --user: {cmd:?}"));
            assert_eq!(cmd[idx + 1], "1000:1000");
        }
    }

    /// `container` only learned `--cap-drop` in 0.12.0 and treats unknown flags
    /// as a fatal parse error, so older versions must get a command without it.
    #[test]
    fn run_omits_cap_drop_when_engine_lacks_it() {
        let caps = EngineCapabilities { cap_drop: false };
        for harness in [Harness::Pi, Harness::Claude, Harness::Codex] {
            let cmd = run_args_with_caps(harness, Backend::Container, caps);
            assert!(
                !cmd.iter().any(|a| a.starts_with("--cap-drop")),
                "command must omit --cap-drop when unsupported: {cmd:?}"
            );
            let idx = cmd
                .iter()
                .position(|a| a == "--user")
                .unwrap_or_else(|| panic!("container command must pass --user: {cmd:?}"));
            assert_eq!(cmd[idx + 1], "1000:1000");
        }
    }

    /// Docker and Podman are never probed; the flag is assumed present.
    #[test]
    fn cap_drop_detection_assumes_support_for_docker_and_podman() {
        for backend in [Backend::Docker, Backend::Podman] {
            assert!(EngineCapabilities::detect("orka-no-such-engine", backend).cap_drop);
        }
    }

    /// A `container` binary that cannot be executed reports no support, which
    /// yields a runnable command rather than one rejected at parse time.
    #[test]
    fn cap_drop_detection_reports_false_for_missing_apple_container() {
        assert!(!EngineCapabilities::detect("orka-no-such-engine", Backend::Container).cap_drop);
    }

    /// Docker and Podman keep `--security-opt=no-new-privileges`; only the
    /// Apple backend drops it.
    #[test]
    fn docker_and_podman_keep_security_opt() {
        for backend in [Backend::Docker, Backend::Podman] {
            let cmd = run_args_for(Harness::Pi, backend);
            assert!(
                cmd.contains(&"--security-opt=no-new-privileges".to_string()),
                "{backend:?} command must pass --security-opt: {cmd:?}"
            );
        }
    }

    /// Build commands are flag-compatible with `container build`, so the same
    /// argv shape is emitted for every engine apart from the leading binary.
    #[test]
    fn apple_container_build_matches_docker_argv_shape() {
        let mut docker_cfg = make_cfg(Harness::Pi);
        docker_cfg.harness_version = Some("1.2.3".to_string());
        let mut container_cfg = make_cfg(Harness::Pi);
        container_cfg.harness_version = Some("1.2.3".to_string());
        container_cfg.backend = Backend::Container;
        container_cfg.engine_binary = "container".to_string();

        let docker_cmd = build_pi_main_command(
            "orka:1.2.3",
            "orka-base:latest",
            "user",
            1000,
            1000,
            "/ctx",
            &docker_cfg,
        );
        let container_cmd = build_pi_main_command(
            "orka:1.2.3",
            "orka-base:latest",
            "user",
            1000,
            1000,
            "/ctx",
            &container_cfg,
        );

        assert_eq!(container_cmd[0], "container");
        assert_eq!(docker_cmd[1..], container_cmd[1..]);
    }

    #[test]
    fn shadow_volumes_rendered_with_ro() {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/root".to_string());
        let pi_dir = format!("{home}/.pi");
        // Ensure ~/.pi exists so run_pi_command_args doesn't fail.
        std::fs::create_dir_all(&pi_dir).unwrap();

        let mut cfg = make_cfg(Harness::Pi);
        cfg.shadow_volumes = vec![("/tmp/empty".to_string(), "/project/.env".to_string())];
        let cmd = run_pi_command_args("orka:latest", 1000, 1000, ALL_CAPS, &cfg).unwrap();
        let joined = cmd.join(" ");
        assert!(joined.contains("/tmp/empty:/project/.env:ro"));
        // Regular volumes must not get :ro.
        assert!(!joined.contains(&format!("{pi_dir}:{pi_dir}:ro")));
    }

    #[test]
    fn pi_build_passes_version_when_set() {
        let mut cfg = make_cfg(Harness::Pi);
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
