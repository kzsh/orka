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
    /// Binary name of the container engine (e.g. "docker", "podman", "nerdctl").
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
    /// Explicit path to the agent binary.  Used only by the bwrap backend;
    /// ignored by all container-engine paths.
    pub harness_binary: Option<String>,
    pub workdir: String,
    pub container_args: Vec<String>,
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
pub fn build_and_run(cfg: &RunConfig) -> Result<(), String> {
    let ctx = write_build_context()?;
    let ctx_path = ctx
        .path()
        .to_str()
        .ok_or_else(|| "temp build context path contains non-UTF-8 characters".to_string())?;

    let base_ref = format!("{BASE_CONTAINER_NAME}:latest");
    let uid = current_uid();
    let gid = current_gid();
    let uname = std::env::var("USER")
        .or_else(|_| std::env::var("LOGNAME"))
        .unwrap_or_else(|_| "appuser".to_string());

    let base_build = build_base_command(&base_ref, ctx_path, cfg);

    let (main_build, run_cmd) = match cfg.harness {
        Harness::Pi => {
            let tag = cfg.harness_version.as_deref().unwrap_or("latest");
            let main_ref = format!("{PI_CONTAINER_NAME}:{tag}");
            let b = build_pi_main_command(&main_ref, &base_ref, &uname, uid, gid, ctx_path, cfg);
            let r = run_pi_command_args(&main_ref, uid, gid, cfg)?;
            (b, r)
        }
        Harness::Claude => {
            let main_ref = format!("{CLAUDE_CONTAINER_NAME}:latest");
            let b =
                build_claude_main_command(&main_ref, &base_ref, &uname, uid, gid, ctx_path, cfg);
            let r = run_claude_command_args(&main_ref, uid, gid, cfg)?;
            (b, r)
        }
        Harness::Codex => {
            let main_ref = format!("{CODEX_CONTAINER_NAME}:latest");
            let b = build_codex_main_command(&main_ref, &base_ref, &uname, uid, gid, ctx_path, cfg);
            let r = run_codex_command_args(&main_ref, uid, gid, cfg)?;
            (b, r)
        }
    };

    if cfg.dry_run {
        print_dry_run("base image build", &base_build);
        print_dry_run("main image build", &main_build);
        print_dry_run("container run", &run_cmd);
        return Ok(());
    }

    if !cfg.verbose {
        let harness_label = match cfg.harness {
            Harness::Pi => "pi",
            Harness::Claude => "claude",
            Harness::Codex => "codex",
        };
        println!("=====================");
        println!("Orka: {harness_label}");
        println!("=====================");
    }

    exec(&base_build)?;
    exec(&main_build)?;
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
    cfg: &RunConfig,
) -> Result<Vec<String>, String> {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/root".to_string());
    let pi_dir = format!("{home}/.pi");
    fs::create_dir_all(&pi_dir).map_err(|e| format!("failed to create {pi_dir}: {e}"))?;

    let mut cmd = vec![
        cfg.engine_binary.clone(),
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
    if matches!(cfg.backend, Backend::Podman) {
        cmd.push(s("--userns=keep-id"));
    }

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
    if matches!(cfg.backend, Backend::Podman) {
        cmd.push(s("--userns=keep-id"));
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
    cfg: &RunConfig,
) -> Result<Vec<String>, String> {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/root".to_string());
    let codex_dir = format!("{home}/.codex");
    fs::create_dir_all(&codex_dir).map_err(|e| format!("failed to create {codex_dir}: {e}"))?;

    let mut cmd = vec![
        cfg.engine_binary.clone(),
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
    if matches!(cfg.backend, Backend::Podman) {
        cmd.push(s("--userns=keep-id"));
    }

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
        let cmd = run_pi_command_args("orka:latest", 1000, 1000, &cfg).unwrap();
        assert!(cmd.contains(&"--userns=keep-id".to_string()));
    }

    #[test]
    fn nerdctl_run_omits_userns_keep_id() {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/root".to_string());
        let pi_dir = format!("{home}/.pi");
        std::fs::create_dir_all(&pi_dir).unwrap();

        let mut cfg = make_cfg(Harness::Pi);
        cfg.engine_binary = "nerdctl".to_string();
        cfg.backend = Backend::Nerdctl;
        let cmd = run_pi_command_args("orka:latest", 1000, 1000, &cfg).unwrap();
        assert!(!cmd.contains(&"--userns=keep-id".to_string()));
    }

    #[test]
    fn docker_run_omits_userns_keep_id() {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/root".to_string());
        let pi_dir = format!("{home}/.pi");
        std::fs::create_dir_all(&pi_dir).unwrap();

        let cfg = make_cfg(Harness::Pi);
        let cmd = run_pi_command_args("orka:latest", 1000, 1000, &cfg).unwrap();
        assert!(!cmd.contains(&"--userns=keep-id".to_string()));
    }

    #[test]
    fn shadow_volumes_rendered_with_ro() {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/root".to_string());
        let pi_dir = format!("{home}/.pi");
        // Ensure ~/.pi exists so run_pi_command_args doesn't fail.
        std::fs::create_dir_all(&pi_dir).unwrap();

        let mut cfg = make_cfg(Harness::Pi);
        cfg.shadow_volumes = vec![("/tmp/empty".to_string(), "/project/.env".to_string())];
        let cmd = run_pi_command_args("orka:latest", 1000, 1000, &cfg).unwrap();
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
