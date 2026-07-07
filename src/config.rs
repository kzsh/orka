use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;

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
    let config_home = std::env::var("XDG_CONFIG_HOME")
        .ok()
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            let home = std::env::var("HOME").unwrap_or_else(|_| "/root".to_string());
            PathBuf::from(home).join(".config")
        });
    config_home.join("orka").join("environments.yaml")
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
}
