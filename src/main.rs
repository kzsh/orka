use std::fs;
use std::path::Path;
use std::process;

use clap::Parser;

mod cli;
mod config;
mod docker;
mod expand;

use cli::Cli;
use docker::RunConfig;

fn main() {
    if let Err(e) = run() {
        eprintln!("orka: {e}");
        process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let cli = Cli::parse();

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

    let workdir = if !cli.file.is_empty() {
        for fp in &cli.file {
            let host_path = resolve_file_path(fp, &cwd)?;
            volumes.push((host_path.clone(), host_path));
        }
        // Multiple files may span different directories; use CWD as a stable
        // anchor. Docker creates the workdir in the container if it doesn't
        // exist as a mount, which is fine — the agent references files by their
        // absolute paths.
        cwd.clone()
    } else if cwd != home {
        volumes.push((cwd.clone(), cwd.clone()));
        cwd.clone()
    } else {
        home.clone()
    };

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

    let run_cfg = RunConfig {
        runtime: cli.runtime,
        no_cache: cli.no_cache,
        dry_run: cli.dry_run,
        quiet: cli.quiet,
        debug: cli.debug,
        ephemeral: cli.ephemeral,
        harness_version: cli.harness_version,
        no_browser: cli.no_browser,
        volumes,
        env_vars,
        workdir,
        container_args: cli.container_args,
    };

    docker::build_and_run(&run_cfg)
}

/// Resolve a `--file` argument to an absolute path string ready to pass to Docker.
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
}
