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

    // Base volumes: current directory (when not running from $HOME) and
    // ~/.agents (when that directory exists).
    let mut volumes: Vec<(String, String)> = Vec::new();

    let workdir = if cwd != home {
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
