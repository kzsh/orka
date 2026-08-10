use std::fs;
use std::path::Path;
use std::process;

use clap::{CommandFactory, FromArgMatches};

mod bwrap;
mod cli;
mod config;
mod docker;
mod expand;
mod scratchpad;
mod shadow;

use cli::{Cli, Commands, ConfigCommand};
use docker::RunConfig;

const LICENSE: &str = include_str!("../LICENSE");
const THIRD_PARTY_LICENSES: &str = include_str!("../THIRD_PARTY_LICENSES");

const TEMPLATE_CONFIG: &str = include_str!("../config/config.yaml");
const TEMPLATE_ENVIRONMENTS: &str = include_str!("../config/environments.yaml");
const TEMPLATE_ORKASHADOW: &str = include_str!("../config/orkashadow");

fn main() {
    if let Err(e) = run() {
        eprintln!("orka: {e}");
        process::exit(1);
    }
}

fn run() -> Result<(), String> {
    // Split argv at `--`: everything before goes to clap, everything after is
    // forwarded verbatim to the container.
    let raw: Vec<String> = std::env::args().collect();
    let (orka_argv, container_args) = match raw.iter().position(|a| a == "--") {
        Some(pos) => (raw[..pos].to_vec(), raw[pos + 1..].to_vec()),
        None => (raw, vec![]),
    };

    let matches = Cli::command()
        .try_get_matches_from(orka_argv)
        .unwrap_or_else(|e| {
            if e.kind() == clap::error::ErrorKind::UnknownArgument {
                let _ = e.print();
                eprintln!("\nnote: to pass arguments to the agent, separate them with '--':");
                eprintln!("  orka [OPTIONS] -- <agent args>");
                std::process::exit(2);
            }
            e.exit()
        });
    let mut cli = Cli::from_arg_matches(&matches).unwrap_or_else(|e| e.exit());

    // --preset list: print available preset names and exit.  Checked before the
    // subcommand is acted on so it works in any argument position.
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

    // `config` is self-contained: it neither reads config.yaml defaults nor
    // starts a container.  `scratchpad` and `tmp` only select the workdir and
    // then fall through to the normal run path.
    let mut scratchpad_name: Option<String> = None;
    let mut use_tmp = false;
    match cli.command {
        Some(Commands::Config { ref command }) => return run_config_command(command),
        Some(Commands::Tmp) => {
            if !cli.file.is_empty() {
                return Err("tmp conflicts with --file".to_string());
            }
            use_tmp = true;
        }
        Some(Commands::Scratchpad { ref name, list }) => {
            let home = std::env::var("HOME").unwrap_or_else(|_| "/root".to_string());
            let xdg = std::env::var("XDG_DATA_HOME").ok();
            if list {
                for name in scratchpad::list(&home, xdg.as_deref())? {
                    println!("{name}");
                }
                return Ok(());
            }
            if !cli.file.is_empty() {
                return Err("scratchpad conflicts with --file".to_string());
            }
            scratchpad_name = Some(match name {
                Some(n) => n.clone(),
                None => select_scratchpad(&home, xdg.as_deref())?,
            });
        }
        None => {}
    }

    // Apply config.yaml defaults for any value the user did not explicitly set.
    let defaults = apply_config_defaults(&mut cli, &matches)?;

    if cli.print_license {
        print!("{LICENSE}");
        println!();
        println!("This binary incorporates third-party open source components.");
        println!("Their license texts are reproduced below for attribution.");
        println!();
        print!("{THIRD_PARTY_LICENSES}");
        return Ok(());
    }

    let home = std::env::var("HOME").unwrap_or_else(|_| "/root".to_string());
    let cwd = std::env::current_dir()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| home.clone());

    // Base volumes: a single file (when --file is given), the current directory
    // (when not running from $HOME), or nothing (when CWD is $HOME).
    // ~/.agents is always appended when it exists.
    let mut volumes: Vec<(String, String)> = Vec::new();
    // Resolved absolute paths of --file mounts; used to build the prompt snippet.
    let mut mounted_files: Vec<String> = Vec::new();

    let workdir = if use_tmp {
        let tmp = make_temp_workdir()?;
        volumes.push((tmp.clone(), tmp.clone()));
        tmp
    } else if let Some(ref name) = scratchpad_name {
        let xdg_data_home = std::env::var("XDG_DATA_HOME").ok();
        let scratch = scratchpad::dir(name, &home, xdg_data_home.as_deref())?;
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
    let mut container_args = container_args;
    if !container_args.is_empty() {
        if let Some(snippet) =
            prompt_context_snippet(use_tmp, scratchpad_name.as_deref(), &mounted_files)
        {
            container_args[0] = format!("{snippet}\n\n{}", container_args[0]);
        }
    }

    // config.yaml harness-args run ahead of the user's `--` arguments so the
    // prompt (which must stay last for claude and codex) is not displaced.
    if let Some(ref harness_args) = defaults.harness_args {
        container_args =
            prepend_harness_args(harness_args.for_harness(cli.harness), container_args);
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
                let from_config = defaults
                    .preset
                    .as_ref()
                    .is_some_and(|p| p.iter().any(|c| c == preset_name));
                unknown_preset_error(
                    preset_name,
                    &available,
                    from_config.then(|| config::defaults_path().display().to_string()),
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

    for raw in &cli.volume {
        let (host_real, container) = resolve_volume_spec(raw)?;
        // Container engines reject two mounts at the same destination, and the
        // working directory or a preset may already cover this path.
        if volumes.iter().any(|(_, dest)| *dest == container) {
            continue;
        }
        volumes.push((host_real, container));
    }

    // Compute shadow mounts from any .orkashadow files found in mounted
    // directories.  _shadow_tmp holds the temp dir containing the empty shadow
    // source file and must stay alive until the container exits.  None means
    // no shadow files were found and no temp dir was created.
    let global_shadow = config::global_shadow_path();
    let (shadow_volumes, _shadow_tmp) = shadow::collect_shadow_volumes(&volumes, &global_shadow)?;

    let harness_binary = match cli.harness {
        cli::Harness::Pi => defaults.pi_path,
        cli::Harness::Claude => defaults.claude_path,
        cli::Harness::Codex => defaults.codex_path,
    };

    let run_cfg = RunConfig {
        engine_binary: cli.engine.binary().to_string(),
        backend: cli.engine,
        harness: cli.harness,
        harness_binary,
        no_cache: cli.no_cache,
        dry_run: cli.dry_run,
        verbose: cli.verbose,
        quiet: cli.quiet,
        preserve_container: cli.preserve_container,
        harness_version: cli.harness_version,
        volumes,
        shadow_volumes,
        env_vars,
        presets: cli.preset.clone(),
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

/// Choose an existing scratchpad interactively.
fn select_scratchpad(home: &str, xdg_data_home: Option<&str>) -> Result<String, String> {
    let names = scratchpad::list(home, xdg_data_home)?;
    if names.is_empty() {
        return Err(format!(
            "no scratchpads exist yet in {}\ncreate one with: orka scratchpad <NAME>",
            scratchpad::root(home, xdg_data_home)
        ));
    }
    scratchpad::pick(&names, "scratchpad> ")?.ok_or_else(|| "no scratchpad selected".to_string())
}

/// Resolve a `--volume` argument into a `(host, container)` pair.
///
/// `HOST:CONTAINER` sets both sides explicitly; a bare path is mounted at the
/// same absolute path it has on the host.  The host side is canonicalized so
/// the engine receives a real path, while the container side keeps the
/// uncanonicalized absolute path, so a symlinked source still appears where
/// the user expects it.
fn resolve_volume_spec(raw: &str) -> Result<(String, String), String> {
    let (host_raw, container_raw) = split_once_colon(raw);
    let host = expand::expand_tilde(host_raw);
    let host_real = fs::canonicalize(&host)
        .map_err(|e| format!("--volume: cannot resolve {host}: {e}"))?
        .to_string_lossy()
        .to_string();

    let container = if container_raw == host_raw {
        std::path::absolute(&host)
            .map_err(|e| format!("--volume: cannot resolve {host}: {e}"))?
            .to_string_lossy()
            .to_string()
    } else {
        let container = expand::expand_tilde(container_raw);
        if !Path::new(&container).is_absolute() {
            return Err(format!(
                "--volume: container path must be absolute: {container}"
            ));
        }
        container
    };

    Ok((host_real, container))
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
fn apply_config_defaults(
    cli: &mut Cli,
    matches: &clap::ArgMatches,
) -> Result<config::Defaults, String> {
    let path = config::defaults_path();
    let defaults = config::load_defaults(&path)?;
    merge_defaults(cli, matches, &defaults);
    Ok(defaults)
}

/// Merge loaded `defaults` into `cli`, leaving anything the user set alone.
fn merge_defaults(cli: &mut Cli, matches: &clap::ArgMatches, defaults: &config::Defaults) {
    use clap::parser::ValueSource;

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

    // Repeatable list flags accumulate: config values come first so CLI
    // additions are layered on top rather than replacing them.
    // Presets are additionally deduplicated: naming an always-on preset again
    // on the command line would otherwise mount the same paths twice, which
    // the container engines reject as a duplicate mount point.
    if let Some(ref presets) = defaults.preset {
        let mut merged = presets.clone();
        merged.append(&mut cli.preset);
        let mut seen = std::collections::HashSet::new();
        merged.retain(|p| seen.insert(p.clone()));
        cli.preset = merged;
    }
    if let Some(ref env) = defaults.env {
        let mut merged = env.clone();
        merged.append(&mut cli.env);
        cli.env = merged;
    }
    if let Some(ref volume) = defaults.volume {
        let mut merged = volume.clone();
        merged.append(&mut cli.volume);
        cli.volume = merged;
    }

    // Boolean flags can only be turned on from the command line, so a config
    // value of true is simply OR-ed in.
    cli.no_cache |= defaults.no_cache.unwrap_or(false);
    cli.verbose |= defaults.verbose.unwrap_or(false);
    cli.quiet |= defaults.quiet.unwrap_or(false);
    cli.preserve_container |= defaults.preserve_container.unwrap_or(false);
}

/// Error text for a preset that is not defined in `environments.yaml`.
///
/// `always_on_source` is the path to the config file when the preset came from
/// its always-on `preset` list, so a stale entry there points at the file to
/// edit instead of looking like a mistyped flag.
fn unknown_preset_error(
    preset_name: &str,
    available: &[&str],
    always_on_source: Option<String>,
) -> String {
    let mut msg = format!(
        "unknown preset: {preset_name}\navailable presets: {}",
        available.join(", ")
    );
    if let Some(path) = always_on_source {
        msg.push_str(&format!(
            "\nnote: '{preset_name}' is listed under 'preset' in {path}; \
             remove it there or define it in the environments file"
        ));
    }
    msg
}

/// Place configured harness arguments ahead of the arguments the user passed
/// after `--`, so a trailing prompt stays trailing.
fn prepend_harness_args(extra: &[String], container_args: Vec<String>) -> Vec<String> {
    if extra.is_empty() {
        return container_args;
    }
    let mut merged = extra.to_vec();
    merged.extend(container_args);
    merged
}

/// Dispatch a `config` subcommand.
fn run_config_command(command: &ConfigCommand) -> Result<(), String> {
    match command {
        ConfigCommand::Init => run_init(),
        ConfigCommand::Completions { shell } => {
            let mut cmd = Cli::command();
            let name = cmd.get_name().to_string();
            clap_complete::generate(*shell, &mut cmd, name, &mut std::io::stdout());
            Ok(())
        }
        ConfigCommand::Path => {
            println!("defaults     {}", config::defaults_path().display());
            println!("environments {}", config::config_path().display());
            println!("shadow       {}", config::global_shadow_path().display());
            println!(
                "dockerfile   {}",
                config::custom_dockerfile_base_path().display()
            );
            println!("scratchpads  {}", scratchpad_root_display());
            Ok(())
        }
    }
}

/// Scratchpad root path as shown by `orka config path`.
fn scratchpad_root_display() -> String {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/root".to_string());
    let xdg = std::env::var("XDG_DATA_HOME").ok();
    scratchpad::root(&home, xdg.as_deref())
}

/// Write the bundled config templates to `~/.config/orka/`.
///
/// Each file is written only when it does not already exist so user
/// customisations are never overwritten.
fn run_init() -> Result<(), String> {
    let cfg_dir = config::config_path()
        .parent()
        .expect("config path has no parent")
        .to_path_buf();

    fs::create_dir_all(&cfg_dir)
        .map_err(|e| format!("failed to create {}: {e}", cfg_dir.display()))?;

    let files: &[(&str, &str)] = &[
        ("config.yaml", TEMPLATE_CONFIG),
        ("environments.yaml", TEMPLATE_ENVIRONMENTS),
        ("orkashadow", TEMPLATE_ORKASHADOW),
    ];

    for (name, content) in files {
        let path = cfg_dir.join(name);
        if path.exists() {
            println!("skipped  {}", path.display());
        } else {
            fs::write(&path, content)
                .map_err(|e| format!("failed to write {}: {e}", path.display()))?;
            println!("wrote    {}", path.display());
        }
    }

    Ok(())
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
    fn volume_spec_bare_path_mounts_at_same_path() {
        let tmp = std::env::temp_dir().join("orka-volume-spec-bare");
        fs::create_dir_all(&tmp).unwrap();
        let path = tmp.to_string_lossy().to_string();
        let (host, container) = resolve_volume_spec(&path).unwrap();
        assert_eq!(container, path);
        assert_eq!(host, fs::canonicalize(&path).unwrap().to_string_lossy());
        fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn volume_spec_explicit_container_path() {
        let tmp = std::env::temp_dir().join("orka-volume-spec-explicit");
        fs::create_dir_all(&tmp).unwrap();
        let spec = format!("{}:/mnt/elsewhere", tmp.display());
        let (_, container) = resolve_volume_spec(&spec).unwrap();
        assert_eq!(container, "/mnt/elsewhere");
        fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn volume_spec_rejects_relative_container_path() {
        let tmp = std::env::temp_dir().join("orka-volume-spec-relative");
        fs::create_dir_all(&tmp).unwrap();
        let spec = format!("{}:elsewhere", tmp.display());
        let err = resolve_volume_spec(&spec).unwrap_err();
        assert!(err.contains("must be absolute"), "{err}");
        fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn volume_spec_rejects_missing_host_path() {
        let err = resolve_volume_spec("/definitely/not/here/orka").unwrap_err();
        assert!(err.contains("cannot resolve"), "{err}");
    }

    /// Build a `Cli` plus its `ArgMatches` the same way `run()` does.
    fn parse_cli(argv: &[&str]) -> (Cli, clap::ArgMatches) {
        let matches = Cli::command().try_get_matches_from(argv).unwrap();
        let cli = Cli::from_arg_matches(&matches).unwrap();
        (cli, matches)
    }

    /// Repeated `--preset` takes exactly one value per occurrence; the words
    /// after it belong to the subcommand.
    #[test]
    fn preset_does_not_consume_the_subcommand() {
        let (cli, _) = parse_cli(&[
            "orka",
            "--preset",
            "linear",
            "--preset",
            "gh",
            "scratchpad",
            "foobar",
        ]);
        assert_eq!(cli.preset, vec!["linear".to_string(), "gh".to_string()]);
        match cli.command {
            Some(Commands::Scratchpad { ref name, list }) => {
                assert_eq!(name.as_deref(), Some("foobar"));
                assert!(!list);
            }
            ref other => panic!("expected scratchpad, got {other:?}"),
        }
    }

    /// Top-level flags are global, so they parse after the subcommand too.
    #[test]
    fn global_flags_parse_after_the_subcommand() {
        let (cli, _) = parse_cli(&[
            "orka",
            "scratchpad",
            "foobar",
            "--preset",
            "linear",
            "--preset",
            "gh",
            "--dry-run",
        ]);
        assert_eq!(cli.preset, vec!["linear".to_string(), "gh".to_string()]);
        assert!(cli.dry_run);
        match cli.command {
            Some(Commands::Scratchpad { ref name, .. }) => {
                assert_eq!(name.as_deref(), Some("foobar"))
            }
            ref other => panic!("expected scratchpad, got {other:?}"),
        }
    }

    /// `scratch` is an alias for `scratchpad`.
    #[test]
    fn scratch_is_an_alias_for_scratchpad() {
        let (cli, _) = parse_cli(&["orka", "scratch", "foobar"]);
        match cli.command {
            Some(Commands::Scratchpad { ref name, .. }) => {
                assert_eq!(name.as_deref(), Some("foobar"))
            }
            ref other => panic!("expected scratchpad, got {other:?}"),
        }
    }

    /// A global flag may also sit between the subcommand and its positional.
    #[test]
    fn global_flags_parse_between_subcommand_and_positional() {
        let (cli, _) = parse_cli(&["orka", "scratchpad", "--preset", "gh", "foobar"]);
        assert_eq!(cli.preset, vec!["gh".to_string()]);
        match cli.command {
            Some(Commands::Scratchpad { ref name, .. }) => {
                assert_eq!(name.as_deref(), Some("foobar"))
            }
            ref other => panic!("expected scratchpad, got {other:?}"),
        }
    }

    #[test]
    fn merge_defaults_fills_unset_scalars() {
        let (mut cli, matches) = parse_cli(&["orka"]);
        let defaults = config::Defaults {
            engine: Some(cli::Backend::Podman),
            harness: Some(cli::Harness::Claude),
            harness_version: Some("1.2.3".to_string()),
            ..Default::default()
        };
        merge_defaults(&mut cli, &matches, &defaults);
        assert_eq!(cli.engine, cli::Backend::Podman);
        assert_eq!(cli.harness, cli::Harness::Claude);
        assert_eq!(cli.harness_version.as_deref(), Some("1.2.3"));
    }

    #[test]
    fn merge_defaults_does_not_override_explicit_flags() {
        let (mut cli, matches) = parse_cli(&[
            "orka",
            "--engine",
            "docker",
            "--harness",
            "pi",
            "-v",
            "9.9.9",
        ]);
        let defaults = config::Defaults {
            engine: Some(cli::Backend::Podman),
            harness: Some(cli::Harness::Claude),
            harness_version: Some("1.2.3".to_string()),
            ..Default::default()
        };
        merge_defaults(&mut cli, &matches, &defaults);
        assert_eq!(cli.engine, cli::Backend::Docker);
        assert_eq!(cli.harness, cli::Harness::Pi);
        assert_eq!(cli.harness_version.as_deref(), Some("9.9.9"));
    }

    #[test]
    fn merge_defaults_appends_list_values_before_cli_ones() {
        let (mut cli, matches) =
            parse_cli(&["orka", "--preset", "go", "--env", "B=2", "--volume", "/b"]);
        let defaults = config::Defaults {
            preset: Some(vec!["rust".to_string()]),
            env: Some(vec!["A=1".to_string()]),
            volume: Some(vec!["/a".to_string()]),
            ..Default::default()
        };
        merge_defaults(&mut cli, &matches, &defaults);
        assert_eq!(cli.preset, vec!["rust".to_string(), "go".to_string()]);
        assert_eq!(cli.env, vec!["A=1".to_string(), "B=2".to_string()]);
        assert_eq!(cli.volume, vec!["/a".to_string(), "/b".to_string()]);
    }

    #[test]
    fn unknown_preset_error_from_cli_has_no_config_note() {
        let msg = unknown_preset_error("jra", &["jira", "rust"], None);
        assert!(msg.contains("unknown preset: jra"));
        assert!(msg.contains("available presets: jira, rust"));
        assert!(!msg.contains("note:"));
    }

    #[test]
    fn unknown_preset_error_from_config_points_at_the_file() {
        let msg = unknown_preset_error(
            "jra",
            &["jira"],
            Some("/home/u/.config/orka/config.yaml".to_string()),
        );
        assert!(msg.contains("/home/u/.config/orka/config.yaml"));
        assert!(msg.contains("remove it there"));
    }

    /// Naming an always-on preset again must not duplicate its mounts.
    #[test]
    fn merge_defaults_deduplicates_presets() {
        let (mut cli, matches) = parse_cli(&["orka", "--preset", "jira", "--preset", "rust"]);
        let defaults = config::Defaults {
            preset: Some(vec!["jira".to_string()]),
            ..Default::default()
        };
        merge_defaults(&mut cli, &matches, &defaults);
        assert_eq!(cli.preset, vec!["jira".to_string(), "rust".to_string()]);
    }

    #[test]
    fn merge_defaults_enables_boolean_flags() {
        let (mut cli, matches) = parse_cli(&["orka"]);
        let defaults = config::Defaults {
            no_cache: Some(true),
            verbose: Some(true),
            quiet: Some(true),
            preserve_container: Some(true),
            ..Default::default()
        };
        merge_defaults(&mut cli, &matches, &defaults);
        assert!(cli.no_cache && cli.verbose && cli.quiet && cli.preserve_container);
    }

    /// A `false` in the config must not undo a flag given on the command line.
    #[test]
    fn merge_defaults_false_does_not_clear_cli_flag() {
        let (mut cli, matches) = parse_cli(&["orka", "--verbose"]);
        let defaults = config::Defaults {
            verbose: Some(false),
            ..Default::default()
        };
        merge_defaults(&mut cli, &matches, &defaults);
        assert!(cli.verbose);
    }

    #[test]
    fn harness_args_precede_the_user_prompt() {
        let extra = vec!["--dangerously-skip-permissions".to_string()];
        let merged = prepend_harness_args(&extra, vec!["fix the build".to_string()]);
        assert_eq!(
            merged,
            vec![
                "--dangerously-skip-permissions".to_string(),
                "fix the build".to_string()
            ]
        );
    }

    #[test]
    fn harness_args_apply_without_user_args() {
        let extra = vec!["--dangerously-skip-permissions".to_string()];
        assert_eq!(prepend_harness_args(&extra, vec![]), extra);
    }

    #[test]
    fn harness_args_empty_leaves_container_args_untouched() {
        let args = vec!["do the thing".to_string()];
        assert_eq!(prepend_harness_args(&[], args.clone()), args);
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
}
