use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::cli::{Backend, Harness};

/// User defaults from `config.yaml`.  Every field is optional; absent fields
/// leave the corresponding CLI default in place.
///
/// Keys are kebab-case (`harness-version`, `pi-path`), matching the shipped
/// template and the documented format.  Unknown keys are rejected so a typo
/// fails loudly instead of being silently ignored.
#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct Defaults {
    /// Default isolation backend (`docker`, `podman`, `bubblewrap`).
    pub engine: Option<Backend>,
    /// Default agent harness (`pi`, `claude`, `codex`).
    pub harness: Option<Harness>,
    /// Default harness version string passed to `--harness-version`.
    pub harness_version: Option<String>,
    /// Explicit path to the pi binary (bwrap backend only).
    /// When absent, pi is located by searching PATH.
    pub pi_path: Option<String>,
    /// Explicit path to the claude binary (bwrap backend only).
    pub claude_path: Option<String>,
    /// Explicit path to the codex binary (bwrap backend only).
    pub codex_path: Option<String>,
    /// Extra arguments appended to the harness command line, per harness.
    /// These land ahead of anything the user passes after `--`, so an explicit
    /// flag still wins for harnesses that honour last-one-wins.
    pub harness_args: Option<HarnessArgs>,
    /// Presets applied on every run, as if passed with `--preset`.
    pub preset: Option<Vec<String>>,
    /// Env vars injected on every run, as if passed with `--env`.
    pub env: Option<Vec<String>>,
    /// Paths mounted on every run, as if passed with `--volume`.
    pub volume: Option<Vec<String>>,
    /// Always rebuild the image, as if `--no-cache` were passed.
    pub no_cache: Option<bool>,
    /// Always pass `VERBOSE=1` into the container, as if `--verbose`.
    pub verbose: Option<bool>,
    /// Always suppress build output, as if `--quiet`.
    pub quiet: Option<bool>,
    /// Always keep the container after exit, as if `--preserve-container`.
    pub preserve_container: Option<bool>,
}

/// Per-harness extra CLI arguments.
#[derive(Debug, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct HarnessArgs {
    #[serde(default)]
    pub pi: Vec<String>,
    #[serde(default)]
    pub claude: Vec<String>,
    #[serde(default)]
    pub codex: Vec<String>,
}

impl HarnessArgs {
    /// Arguments configured for `harness`.
    pub fn for_harness(&self, harness: Harness) -> &[String] {
        match harness {
            Harness::Pi => &self.pi,
            Harness::Claude => &self.claude,
            Harness::Codex => &self.codex,
        }
    }
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

/// Returns the canonical path to an optional user-supplied `Dockerfile.base`,
/// honouring `$XDG_CONFIG_HOME`.
pub fn custom_dockerfile_base_path() -> PathBuf {
    orka_config_dir().join("Dockerfile.base")
}

/// Load `config.yaml`.  Returns `Ok(Defaults::default())` when the file does
/// not exist so callers do not need to special-case a missing file.
pub fn load_defaults(path: &Path) -> Result<Defaults, String> {
    if !path.exists() {
        return Ok(Defaults::default());
    }
    let content =
        fs::read_to_string(path).map_err(|e| format!("failed to read {}: {e}", path.display()))?;
    parse(&content, path)
}

/// Deserialize one of our YAML config files.
///
/// A document holding no values -- empty, or comments only, as the shipped
/// templates are -- means "use the defaults".  YAML resolves that to null,
/// which a parser may legitimately refuse to treat as a mapping, so the policy
/// is enforced here instead of relying on parser leniency.
fn parse<T: serde::de::DeserializeOwned + Default + 'static>(
    content: &str,
    path: &Path,
) -> Result<T, String> {
    if content.lines().all(|line| {
        let t = line.trim();
        t.is_empty() || t.starts_with('#')
    }) {
        return Ok(T::default());
    }
    noyalib::from_str(content).map_err(|e| format!("failed to parse {}: {e}", path.display()))
}

/// Top-level structure of `environments.yaml`.
#[derive(Debug, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct Config {
    #[serde(default)]
    pub environments: HashMap<String, Environment>,
}

/// A named environment preset: volumes to mount and env vars to inject.
#[derive(Debug, Deserialize, Default)]
#[serde(deny_unknown_fields)]
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
    parse(&content, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Parse via the production entry point so tests cover the behaviour orka
    /// actually has, not the raw YAML crate's.
    fn try_parse<T: serde::de::DeserializeOwned + Default + 'static>(
        yaml: &str,
    ) -> Result<T, String> {
        parse(yaml, Path::new("<test>"))
    }

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
        let cfg: Config = try_parse(yaml).unwrap();
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

    /// `env` is optional; most presets in the shipped template omit it.
    #[test]
    fn parse_missing_optional_fields() {
        let yaml = "environments:\n  bare:\n    volumes:\n      - /a:/a\n";
        let cfg: Config = try_parse(yaml).unwrap();
        let bare = &cfg.environments["bare"];
        assert_eq!(bare.volumes.len(), 1);
        assert!(bare.env.is_empty());
    }

    /// An environment key with no body is a truncated edit, not an empty
    /// preset, and is rejected rather than silently treated as defaults.
    #[test]
    fn null_bodied_environment_is_rejected() {
        let err = try_parse::<Config>("environments:\n  bare:\n").unwrap_err();
        assert!(err.contains("failed to parse"), "got: {err}");
    }

    #[test]
    fn parse_empty_document() {
        let cfg: Config = try_parse("").unwrap();
        assert!(cfg.environments.is_empty());
    }

    #[test]
    fn parse_defaults_full() {
        let yaml = "engine: podman\nharness: claude\nharness-version: 1.2.3\n";
        let d: Defaults = try_parse(yaml).unwrap();
        assert_eq!(d.engine, Some(Backend::Podman));
        assert_eq!(d.harness, Some(Harness::Claude));
        assert_eq!(d.harness_version.as_deref(), Some("1.2.3"));
    }

    #[test]
    fn parse_defaults_partial() {
        let yaml = "engine: podman\n";
        let d: Defaults = try_parse(yaml).unwrap();
        assert_eq!(d.engine, Some(Backend::Podman));
        assert!(d.harness.is_none());
        assert!(d.harness_version.is_none());
    }

    #[test]
    fn parse_defaults_bubblewrap() {
        let yaml = "engine: bubblewrap\n";
        let d: Defaults = try_parse(yaml).unwrap();
        assert_eq!(d.engine, Some(Backend::Bubblewrap));
    }

    #[test]
    fn parse_defaults_empty_document() {
        let d: Defaults = try_parse("").unwrap();
        assert!(d.engine.is_none());
        assert!(d.harness.is_none());
        assert!(d.harness_version.is_none());
    }

    /// The shipped template is entirely comments.  Copying it into place must
    /// yield defaults, not a parse error that stops orka from starting.
    #[test]
    fn shipped_template_parses_to_defaults() {
        const TEMPLATE: &str = include_str!("../config/config.yaml");
        assert!(
            TEMPLATE
                .lines()
                .all(|l| l.trim().is_empty() || l.trim_start().starts_with('#')),
            "template is expected to be comments only; update this test if it gains values"
        );

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.yaml");
        std::fs::write(&path, TEMPLATE).unwrap();
        let d = load_defaults(&path).unwrap();
        assert!(d.engine.is_none());
        assert!(d.harness.is_none());
        assert!(d.harness_version.is_none());
        assert!(d.pi_path.is_none());
    }

    /// Every key the README and template document must actually bind.  These
    /// are kebab-case; snake_case field names alone silently dropped four of
    /// them.
    #[test]
    fn all_documented_kebab_keys_bind() {
        let yaml = concat!(
            "engine: bubblewrap\n",
            "harness: codex\n",
            "harness-version: 1.2.3\n",
            "pi-path: /opt/pi/bin/pi\n",
            "claude-path: /opt/claude/bin/claude\n",
            "codex-path: /opt/codex/bin/codex\n",
        );
        let d: Defaults = try_parse(yaml).unwrap();
        assert_eq!(d.engine, Some(Backend::Bubblewrap));
        assert_eq!(d.harness, Some(Harness::Codex));
        assert_eq!(d.harness_version.as_deref(), Some("1.2.3"));
        assert_eq!(d.pi_path.as_deref(), Some("/opt/pi/bin/pi"));
        assert_eq!(d.claude_path.as_deref(), Some("/opt/claude/bin/claude"));
        assert_eq!(d.codex_path.as_deref(), Some("/opt/codex/bin/codex"));
    }

    #[test]
    fn parse_harness_args_per_harness() {
        let yaml = concat!(
            "harness-args:\n",
            "  claude:\n",
            "    - --dangerously-skip-permissions\n",
            "  codex:\n",
            "    - --dangerously-bypass-approvals-and-sandbox\n",
        );
        let d: Defaults = try_parse(yaml).unwrap();
        let args = d.harness_args.unwrap();
        assert_eq!(
            args.for_harness(Harness::Claude),
            ["--dangerously-skip-permissions"]
        );
        assert_eq!(
            args.for_harness(Harness::Codex),
            ["--dangerously-bypass-approvals-and-sandbox"]
        );
        assert!(args.for_harness(Harness::Pi).is_empty());
    }

    #[test]
    fn unknown_harness_in_harness_args_is_rejected() {
        assert!(try_parse::<Defaults>("harness-args:\n  gemini:\n    - --yolo\n").is_err());
    }

    #[test]
    fn parse_flag_and_list_defaults() {
        let yaml = concat!(
            "preset:\n  - rust\n  - go\n",
            "env:\n  - RUST_LOG=debug\n",
            "no-cache: true\n",
            "verbose: true\n",
            "quiet: false\n",
            "preserve-container: true\n",
        );
        let d: Defaults = try_parse(yaml).unwrap();
        assert_eq!(
            d.preset.as_deref(),
            Some(&["rust".to_string(), "go".to_string()][..])
        );
        assert_eq!(d.env.as_deref(), Some(&["RUST_LOG=debug".to_string()][..]));
        assert_eq!(d.no_cache, Some(true));
        assert_eq!(d.verbose, Some(true));
        assert_eq!(d.quiet, Some(false));
        assert_eq!(d.preserve_container, Some(true));
    }

    /// snake_case is not the documented format and must not silently work.
    #[test]
    fn snake_case_key_is_rejected() {
        let err = try_parse::<Defaults>("harness_version: 1.2.3\n").unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("harness_version") || msg.contains("unknown"),
            "expected an unknown-key error, got: {msg}"
        );
    }

    #[test]
    fn typo_in_key_is_rejected_not_ignored() {
        assert!(try_parse::<Defaults>("engin: podman\n").is_err());
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
