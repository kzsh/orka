use std::fs;
use std::path::Path;
use std::process;

use clap::{CommandFactory, FromArgMatches};

mod bwrap;
mod cli;
mod config;
mod docker;
mod expand;
mod shadow;

use cli::Cli;
use docker::RunConfig;

const LICENSE: &str = include_str!("../PUBLIC-LICENSE");

fn main() {
    if let Err(e) = run() {
        eprintln!("orka: {e}");
        process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let matches = Cli::command().get_matches();
    let mut cli = Cli::from_arg_matches(&matches).unwrap_or_else(|e| e.exit());

    // Apply config.yaml defaults for any value the user did not explicitly set.
    let defaults = apply_config_defaults(&mut cli, &matches)?;

    if cli.print_license {
        print!("{LICENSE}");
        return Ok(());
    }

    let home = std::env::var("HOME").unwrap_or_else(|_| "/root".to_string());
    let cwd = std::env::current_dir()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| home.clone());

    // --preset list: print available preset names and exit.
    if cli.preset.iter().any(|p| p == "list") {
        let cfg_path = config::config_path();
        require_config_file(&cfg_path)?;
        let cfg = config::load(&cfg_path)?;
        let mut names: Vec<&str> = cfg.environments.keys().map(String::as_str).collect();
        names.sort_unstable();
        for name in names {
            println!("{name}");
        }
        return Ok(());
    }

    // Base volumes: a single file (when --file is given), the current directory
    // (when not running from $HOME), or nothing (when CWD is $HOME).
    // ~/.agents is always appended when it exists.
    let mut volumes: Vec<(String, String)> = Vec::new();
    // Resolved absolute paths of --file mounts; used to build the prompt snippet.
    let mut mounted_files: Vec<String> = Vec::new();

    let workdir = if cli.tmp {
        let tmp = make_temp_workdir()?;
        volumes.push((tmp.clone(), tmp.clone()));
        tmp
    } else if let Some(ref name) = cli.scratchpad {
        let scratch = scratchpad_dir(name, &home)?;
        volumes.push((scratch.clone(), scratch.clone()));
        scratch
    } else if !cli.file.is_empty() {
        for fp in &cli.file {
            let host_path = resolve_file_path(fp, &cwd)?;
            volumes.push((host_path.clone(), host_path.clone()));
            mounted_files.push(host_path);
        }
        // Multiple files may span different directories; use CWD as a stable
        // anchor. The engine creates the workdir in the container if it doesn't
        // exist as a mount, which is fine — the agent references files by their
        // absolute paths.
        cwd.clone()
    } else if cwd != home {
        volumes.push((cwd.clone(), cwd.clone()));
        cwd.clone()
    } else {
        home.clone()
    };

    // Prepend a context snippet to the task prompt so the agent understands the
    // constraints of this environment.  Only injected when a task is present;
    // interactive sessions (no container_args) don't need it.
    let mut container_args = cli.container_args;
    if !container_args.is_empty() {
        if let Some(snippet) =
            prompt_context_snippet(cli.tmp, cli.scratchpad.as_deref(), &mounted_files)
        {
            container_args[0] = format!("{snippet}\n\n{}", container_args[0]);
        }
    }

    let agents_dir = format!("{home}/.agents");
    if Path::new(&agents_dir).is_dir() {
        volumes.push((agents_dir.clone(), agents_dir));
    }

    // Collect volumes and env vars from all presets (if any), then from --env flags.
    let mut env_vars: Vec<(String, String)> = Vec::new();

    if !cli.preset.is_empty() {
        let cfg_path = config::config_path();
        require_config_file(&cfg_path)?;
        let cfg = config::load(&cfg_path)?;

        for preset_name in &cli.preset {
            let env = cfg.environments.get(preset_name).ok_or_else(|| {
                let mut available: Vec<&str> =
                    cfg.environments.keys().map(String::as_str).collect();
                available.sort_unstable();
                format!(
                    "unknown preset: {preset_name}\navailable presets: {}",
                    available.join(", ")
                )
            })?;

            for raw in &env.volumes {
                let (host_raw, container_raw) = split_once_colon(raw);
                let host = expand::expand_tilde(host_raw);
                let container = expand::expand_tilde(container_raw);
                let host_real = fs::canonicalize(&host)
                    .map_err(|_| format!("preset volume path does not exist: {host}"))?
                    .to_string_lossy()
                    .to_string();
                volumes.push((host_real, container));
            }

            for raw in &env.env {
                let (key, val_raw) = split_once_eq(raw);
                env_vars.push((key.to_string(), expand::expand_value(val_raw)));
            }
        }
    }

    for raw in &cli.env {
        let (key, val_raw) = split_once_eq(raw);
        env_vars.push((key.to_string(), expand::expand_value(val_raw)));
    }

    // Compute shadow mounts from any .orkashadow files found in mounted
    // directories.  _shadow_tmp holds the temp dir containing the empty shadow
    // source file and must stay alive until the container exits.  None means
    // no shadow files were found and no temp dir was created.
    let global_shadow = config::global_shadow_path();
    let (shadow_volumes, _shadow_tmp) = shadow::collect_shadow_volumes(&volumes, &global_shadow)?;

    // no_browser is passed through; tmp/scratchpad are already resolved into
    // volumes and workdir above.
    let harness_binary = match cli.harness {
        cli::Harness::Pi => defaults.pi_path,
        cli::Harness::Claude => defaults.claude_path,
        cli::Harness::Codex => defaults.codex_path,
    };

    let run_cfg = RunConfig {
        engine_binary: cli.engine.binary().to_string(),
        harness: cli.harness,
        harness_binary,
        no_cache: cli.no_cache,
        dry_run: cli.dry_run,
        verbose: cli.verbose,
        preserve_container: cli.preserve_container,
        harness_version: cli.harness_version,
        no_browser: cli.no_browser,
        volumes,
        shadow_volumes,
        env_vars,
        workdir,
        container_args,
    };

    if cli.engine.is_bwrap() {
        bwrap::run(&run_cfg)
    } else {
        docker::build_and_run(&run_cfg)
    }
}

/// Build a context snippet that is prepended to the agent's task prompt
/// whenever the invocation uses a constrained mount mode.
///
/// Returns `None` for the normal CWD-mount case where no extra context is needed.
fn prompt_context_snippet(
    is_tmp: bool,
    scratchpad: Option<&str>,
    mounted_files: &[String],
) -> Option<String> {
    const RESTRICTED: &str = "You are running inside a container with a restricted capability set \
         (all Linux capabilities dropped, no new privileges).";

    if is_tmp || scratchpad.is_some() {
        Some(RESTRICTED.to_string())
    } else if !mounted_files.is_empty() {
        let list = mounted_files
            .iter()
            .map(|f| format!("  - {f}"))
            .collect::<Vec<_>>()
            .join("\n");
        Some(format!(
            "{RESTRICTED} \
             Only the following specific files from the host are mounted into the container:\n\
             {list}\n\
             These files are available at their original absolute paths. \
             You may write additional files to those directories, but anything not in the list \
             above will not be persisted once the container exits."
        ))
    } else {
        None
    }
}

/// Create a temporary directory via `mktemp -d` and return its path.
///
/// The directory is not cleaned up automatically; it persists after the
/// container exits so the user can inspect any output left there.
fn make_temp_workdir() -> Result<String, String> {
    let out = std::process::Command::new("mktemp")
        .arg("-d")
        .output()
        .map_err(|e| format!("mktemp -d failed to launch: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "mktemp -d exited with status {}",
            out.status.code().unwrap_or(-1)
        ));
    }
    let path =
        String::from_utf8(out.stdout).map_err(|e| format!("mktemp -d output is not UTF-8: {e}"))?;
    Ok(path.trim().to_string())
}

/// Resolve and create (if necessary) the named scratchpad directory.
///
/// Follows the XDG Base Directory Specification: respects `$XDG_DATA_HOME`
/// when set, otherwise falls back to `$HOME/.local/share`.
/// Path: `$XDG_DATA_HOME/orka/scratch/<name>`
fn scratchpad_dir(name: &str, home: &str) -> Result<String, String> {
    let data_home = std::env::var("XDG_DATA_HOME")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| format!("{home}/.local/share"));
    let path = format!("{data_home}/orka/scratch/{name}");
    fs::create_dir_all(&path)
        .map_err(|e| format!("failed to create scratchpad directory {path}: {e}"))?;
    Ok(path)
}

/// Resolve a `--file` argument to an absolute path string ready to pass to the container engine.
///
/// `cwd` is used as the base for relative paths.
fn resolve_file_path(file_path: &Path, cwd: &str) -> Result<String, String> {
    let candidate = if file_path.is_absolute() {
        file_path.to_path_buf()
    } else {
        Path::new(cwd).join(file_path)
    };

    let abs = fs::canonicalize(&candidate)
        .map_err(|e| format!("--file: cannot resolve {}: {e}", candidate.display()))?;

    if !abs.is_file() {
        return Err(format!("--file: not a regular file: {}", abs.display()));
    }

    Ok(abs.to_string_lossy().to_string())
}

fn require_config_file(path: &Path) -> Result<(), String> {
    if !path.exists() {
        return Err(format!(
            "no environments file found at: {path}\n\
             copy the template to get started:\n  \
             cp <orka_source>/config/environments.yaml {path}",
            path = path.display()
        ));
    }
    Ok(())
}

/// Split `"host:container"` at the first `:`.  If there is no `:`, both halves
/// are the same string (mirrors the bash `expand_tilde` approach).
fn split_once_colon(s: &str) -> (&str, &str) {
    match s.find(':') {
        Some(pos) => (&s[..pos], &s[pos + 1..]),
        None => (s, s),
    }
}

/// Split `"KEY=VALUE"` at the first `=`.  If there is no `=`, the value is
/// treated as empty.
fn split_once_eq(s: &str) -> (&str, &str) {
    match s.find('=') {
        Some(pos) => (&s[..pos], &s[pos + 1..]),
        None => (s, ""),
    }
}

/// Fill in `cli` fields that the user did not explicitly set on the command
/// line, using values from `~/.config/orka/config.yaml`.
///
/// Detection relies on `ArgMatches::value_source`: a field whose source is
/// `ValueSource::DefaultValue` was not supplied by the user, so the config
/// value wins.  Fields supplied on the command line or via environment
/// variables are left untouched.
fn apply_config_defaults(cli: &mut Cli, matches: &clap::ArgMatches) -> Result<config::Defaults, String> {
    use clap::parser::ValueSource;

    let path = config::defaults_path();
    let defaults = config::load_defaults(&path)?;

    let src = |id: &str| matches.value_source(id);
    let is_default = |id: &str| src(id) == Some(ValueSource::DefaultValue);

    if is_default("engine") {
        if let Some(v) = defaults.engine {
            cli.engine = v;
        }
    }

    if is_default("harness") {
        if let Some(v) = defaults.harness {
            cli.harness = v;
        }
    }

    // harness-version is Option<String>: None means the user never set it.
    if cli.harness_version.is_none() {
        if let Some(ref v) = defaults.harness_version {
            cli.harness_version = Some(v.clone());
        }
    }

    if is_default("no-browser") {
        if let Some(v) = defaults.no_browser {
            cli.no_browser = v;
        }
    }

    Ok(defaults)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;

    #[test]
    fn resolve_file_path_absolute() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("foo.txt");
        File::create(&file).unwrap();

        let result = resolve_file_path(&file, "/irrelevant").unwrap();
        assert_eq!(result, file.to_string_lossy());
    }

    #[test]
    fn resolve_file_path_relative() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("bar.txt");
        File::create(&file).unwrap();

        let cwd = dir.path().to_str().unwrap();
        let result = resolve_file_path(Path::new("bar.txt"), cwd).unwrap();
        assert_eq!(result, file.to_string_lossy());
    }

    #[test]
    fn resolve_file_path_nonexistent_returns_err() {
        let result = resolve_file_path(Path::new("/no/such/file.txt"), "/tmp");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("--file:"));
    }

    #[test]
    fn resolve_file_path_directory_returns_err() {
        let dir = tempfile::tempdir().unwrap();
        let result = resolve_file_path(dir.path(), "/tmp");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("not a regular file"));
    }

    #[test]
    fn resolve_file_path_multiple_files_same_dir() {
        let dir = tempfile::tempdir().unwrap();
        let f1 = dir.path().join("a.txt");
        let f2 = dir.path().join("b.txt");
        File::create(&f1).unwrap();
        File::create(&f2).unwrap();

        let r1 = resolve_file_path(&f1, "/irrelevant").unwrap();
        let r2 = resolve_file_path(&f2, "/irrelevant").unwrap();
        assert_eq!(r1, f1.to_string_lossy());
        assert_eq!(r2, f2.to_string_lossy());
        // Both paths should share the same parent — they're valid independent mounts.
        assert_ne!(r1, r2);
    }

    #[test]
    fn split_colon_normal() {
        assert_eq!(
            split_once_colon("~/.cargo:~/.cargo"),
            ("~/.cargo", "~/.cargo")
        );
    }

    #[test]
    fn split_colon_no_colon() {
        assert_eq!(split_once_colon("/some/path"), ("/some/path", "/some/path"));
    }

    #[test]
    fn split_colon_multiple_colons() {
        // Only the first colon is the delimiter; rest stays in the container half.
        assert_eq!(split_once_colon("a:b:c"), ("a", "b:c"));
    }

    #[test]
    fn split_eq_normal() {
        assert_eq!(split_once_eq("KEY=VALUE"), ("KEY", "VALUE"));
    }

    #[test]
    fn split_eq_value_contains_equals() {
        // Handles values like PATH=/usr/bin:/usr/local/bin or BASE64 strings.
        assert_eq!(
            split_once_eq("PATH=/usr/bin=extra"),
            ("PATH", "/usr/bin=extra")
        );
    }

    #[test]
    fn split_eq_no_equals() {
        assert_eq!(split_once_eq("NOVALUE"), ("NOVALUE", ""));
    }

    #[test]
    fn prompt_context_snippet_none_for_normal_mode() {
        assert!(prompt_context_snippet(false, None, &[]).is_none());
    }

    #[test]
    fn prompt_context_snippet_tmp_contains_restricted_notice() {
        let s = prompt_context_snippet(true, None, &[]).unwrap();
        assert!(s.contains("restricted capability set"));
        // Should be brief — no file list noise.
        assert!(!s.contains("mounted"));
    }

    #[test]
    fn prompt_context_snippet_scratchpad_contains_restricted_notice() {
        let s = prompt_context_snippet(false, Some("my-pad"), &[]).unwrap();
        assert!(s.contains("restricted capability set"));
        assert!(!s.contains("mounted"));
    }

    #[test]
    fn prompt_context_snippet_file_lists_paths_and_warns_persistence() {
        let files = vec![
            "/home/user/foo.rs".to_string(),
            "/home/user/bar.rs".to_string(),
        ];
        let s = prompt_context_snippet(false, None, &files).unwrap();
        assert!(s.contains("restricted capability set"));
        assert!(s.contains("/home/user/foo.rs"));
        assert!(s.contains("/home/user/bar.rs"));
        assert!(s.contains("not be persisted"));
    }

    #[test]
    fn make_temp_workdir_returns_existing_dir() {
        let path = make_temp_workdir().unwrap();
        let p = std::path::Path::new(&path);
        assert!(p.exists(), "mktemp -d path does not exist: {path}");
        assert!(p.is_dir(), "mktemp -d path is not a directory: {path}");
        // Clean up so we don't litter /tmp.
        fs::remove_dir(&path).unwrap();
    }

    #[test]
    fn scratchpad_dir_creates_directory() {
        // XDG_DATA_HOME unset → falls back to $HOME/.local/share
        let base = tempfile::tempdir().unwrap();
        let home = base.path().to_str().unwrap();
        std::env::remove_var("XDG_DATA_HOME");
        let result = scratchpad_dir("test-pad", home).unwrap();
        let expected = format!("{home}/.local/share/orka/scratch/test-pad");
        assert_eq!(result, expected);
        assert!(std::path::Path::new(&result).is_dir());
    }

    #[test]
    fn scratchpad_dir_is_idempotent() {
        let base = tempfile::tempdir().unwrap();
        let home = base.path().to_str().unwrap();
        let r1 = scratchpad_dir("my-pad", home).unwrap();
        let r2 = scratchpad_dir("my-pad", home).unwrap();
        assert_eq!(r1, r2);
        assert!(std::path::Path::new(&r1).is_dir());
    }

    #[test]
    fn scratchpad_dir_honours_xdg_data_home() {
        let base = tempfile::tempdir().unwrap();
        let xdg = base.path().join("xdg");
        // Temporarily override XDG_DATA_HOME.  Tests run in the same process so
        // we restore it afterwards to avoid polluting other tests.
        let prev = std::env::var("XDG_DATA_HOME").ok();
        std::env::set_var("XDG_DATA_HOME", &xdg);
        let result = scratchpad_dir("xpad", "/irrelevant");
        match prev {
            Some(v) => std::env::set_var("XDG_DATA_HOME", v),
            None => std::env::remove_var("XDG_DATA_HOME"),
        }
        let path = result.unwrap();
        let expected = format!("{}/orka/scratch/xpad", xdg.display());
        assert_eq!(path, expected);
        assert!(std::path::Path::new(&path).is_dir());
    }
}
