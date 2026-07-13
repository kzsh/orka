use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::cli::{Backend, Harness};

/// User defaults from `config.yaml`.  Every field is optional; absent fields
/// leave the corresponding CLI default in place.
#[derive(Debug, Deserialize, Default)]
pub struct Defaults {
    /// Default isolation backend (`docker`, `podman`, `nerdctl`, `bubblewrap`).
    pub engine: Option<Backend>,
    /// Default agent harness (`pi`, `claude`, `codex`).
    pub harness: Option<Harness>,
    /// Default harness version string passed to `--harness-version`.
    pub harness_version: Option<String>,
    /// When `true`, skips installing the agent-browser extension by default.
    pub no_browser: Option<bool>,
    /// Explicit path to the pi binary (bwrap backend only).
    /// When absent, pi is located by searching PATH.
    pub pi_path: Option<String>,
    /// Explicit path to the claude binary (bwrap backend only).
    pub claude_path: Option<String>,
    /// Explicit path to the codex binary (bwrap backend only).
    pub codex_path: Option<String>,
}

/// Returns the canonical path to `config.yaml`, honouring `$XDG_CONFIG_HOME`.
pub fn defaults_path() -> PathBuf {
    orka_config_dir().join("config.yaml")
}

/// Returns the canonical path to the global `orkashadow` file,
/// honouring `$XDG_CONFIG_HOME`.
pub fn global_shadow_path() -> PathBuf {
    orka_config_dir().join("orkashadow")
}

/// Load `config.yaml`.  Returns `Ok(Defaults::default())` when the file does
/// not exist so callers do not need to special-case a missing file.
pub fn load_defaults(path: &Path) -> Result<Defaults, String> {
    if !path.exists() {
        return Ok(Defaults::default());
    }
    let content =
        fs::read_to_string(path).map_err(|e| format!("failed to read {}: {e}", path.display()))?;
    serde_yml::from_str(&content).map_err(|e| format!("failed to parse {}: {e}", path.display()))
}

/// Top-level structure of `environments.yaml`.
#[derive(Debug, Deserialize, Default)]
pub struct Config {
    #[serde(default)]
    pub environments: HashMap<String, Environment>,
}

/// A named environment preset: volumes to mount and env vars to inject.
#[derive(Debug, Deserialize, Default)]
pub struct Environment {
    /// `host_path:container_path` pairs, same format as `docker --volume`.
    #[serde(default)]
    pub volumes: Vec<String>,

    /// `KEY=VALUE` pairs, same format as `docker --env`.
    #[serde(default)]
    pub env: Vec<String>,
}

/// Returns the canonical path to the environments config file,
/// honouring `$XDG_CONFIG_HOME` when set.
pub fn config_path() -> PathBuf {
    orka_config_dir().join("environments.yaml")
}

/// Returns `$XDG_CONFIG_HOME/orka` (or `~/.config/orka` as the fallback).
fn orka_config_dir() -> PathBuf {
    let config_home = std::env::var("XDG_CONFIG_HOME")
        .ok()
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            let home = std::env::var("HOME").unwrap_or_else(|_| "/root".to_string());
            PathBuf::from(home).join(".config")
        });
    config_home.join("orka")
}

/// Load and parse the environments config file.
pub fn load(path: &Path) -> Result<Config, String> {
    let content =
        fs::read_to_string(path).map_err(|e| format!("failed to read {}: {e}", path.display()))?;
    serde_yml::from_str(&content).map_err(|e| format!("failed to parse {}: {e}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_full_config() {
        let yaml = r#"
environments:
  rust:
    volumes:
      - ~/.cargo/:~/.cargo/
      - ~/.rustup/:~/.rustup/
  go:
    volumes:
      - /usr/local/go:/usr/local/go
    env:
      - PATH=/usr/local/go/bin:$PATH
  empty:
    volumes: []
"#;
        let cfg: Config = serde_yml::from_str(yaml).unwrap();
        assert!(cfg.environments.contains_key("rust"));
        assert!(cfg.environments.contains_key("go"));
        assert!(cfg.environments.contains_key("empty"));

        let rust = &cfg.environments["rust"];
        assert_eq!(rust.volumes.len(), 2);
        assert!(rust.env.is_empty());

        let go = &cfg.environments["go"];
        assert_eq!(go.volumes.len(), 1);
        assert_eq!(go.env.len(), 1);
    }

    #[test]
    fn parse_missing_optional_fields() {
        // `volumes` and `env` both default to empty vec when absent.
        let yaml = "environments:\n  bare:\n";
        let cfg: Config = serde_yml::from_str(yaml).unwrap();
        let bare = &cfg.environments["bare"];
        assert!(bare.volumes.is_empty());
        assert!(bare.env.is_empty());
    }

    #[test]
    fn parse_empty_document() {
        let cfg: Config = serde_yml::from_str("").unwrap();
        assert!(cfg.environments.is_empty());
    }

    #[test]
    fn parse_defaults_full() {
        let yaml = "engine: podman\nharness: claude\nharness-version: 1.2.3\nno_browser: true\n";
        let d: Defaults = serde_yml::from_str(yaml).unwrap();
        assert_eq!(d.engine, Some(Backend::Podman));
        assert_eq!(d.harness, Some(Harness::Claude));
        assert_eq!(d.harness_version.as_deref(), Some("1.2.3"));
        assert_eq!(d.no_browser, Some(true));
    }

    #[test]
    fn parse_defaults_partial() {
        let yaml = "engine: nerdctl\n";
        let d: Defaults = serde_yml::from_str(yaml).unwrap();
        assert_eq!(d.engine, Some(Backend::Nerdctl));
        assert!(d.harness.is_none());
        assert!(d.harness_version.is_none());
        assert!(d.no_browser.is_none());
    }

    #[test]
    fn parse_defaults_bubblewrap() {
        let yaml = "engine: bubblewrap\n";
        let d: Defaults = serde_yml::from_str(yaml).unwrap();
        assert_eq!(d.engine, Some(Backend::Bubblewrap));
    }

    #[test]
    fn parse_defaults_empty_document() {
        let d: Defaults = serde_yml::from_str("").unwrap();
        assert!(d.engine.is_none());
        assert!(d.harness.is_none());
        assert!(d.harness_version.is_none());
        assert!(d.no_browser.is_none());
    }

    #[test]
    fn load_defaults_returns_empty_when_file_missing() {
        let path = std::path::Path::new("/tmp/orka-nonexistent-config-xyz.yaml");
        let d = load_defaults(path).unwrap();
        assert!(d.engine.is_none());
    }

    #[test]
    fn load_defaults_reads_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.yaml");
        std::fs::write(&path, "engine: podman\nharness: codex\n").unwrap();
        let d = load_defaults(&path).unwrap();
        assert_eq!(d.engine, Some(Backend::Podman));
        assert_eq!(d.harness, Some(Harness::Codex));
    }
}
