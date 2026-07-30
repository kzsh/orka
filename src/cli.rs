use clap::{Parser, Subcommand, ValueEnum};
use clap_complete::Shell;
use serde::Deserialize;

/// Isolation backend to use for sandboxing the agent.
///
/// Container engines (docker, podman) build an OCI image and run it.
/// Bubblewrap skips the image entirely and directly namespaces the host
/// filesystem — lighter weight but Linux-only.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, ValueEnum, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Backend {
    #[default]
    Docker,
    Podman,
    /// Lightweight Linux-only sandbox; no image build step required.
    Bubblewrap,
}

impl Backend {
    /// Returns the binary name for container-engine backends.
    /// Not meaningful for [`Backend::Bubblewrap`].
    pub fn binary(self) -> &'static str {
        match self {
            Backend::Docker => "docker",
            Backend::Podman => "podman",
            Backend::Bubblewrap => "bwrap",
        }
    }

    /// Returns true when the backend is bubblewrap (no container image needed).
    pub fn is_bwrap(self) -> bool {
        self == Backend::Bubblewrap
    }
}

/// Which agent harness to launch inside the container.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, ValueEnum, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Harness {
    /// pi coding agent (default)
    #[default]
    Pi,
    /// Anthropic claude-code
    Claude,
    /// OpenAI codex
    Codex,
}

#[derive(Parser, Debug)]
#[command(name = "orka", about = "Agent harness container wrapper", version)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Commands>,

    /// Isolation backend.  Container engines (docker/podman) build an OCI image;
    /// bubblewrap sandboxes the host filesystem directly (Linux only, no image
    /// build required).
    #[arg(long, value_enum, default_value = "docker")]
    pub engine: Backend,

    /// Agent harness to use inside the container.
    #[arg(long, value_enum, default_value = "pi")]
    pub harness: Harness,

    /// Select a named preset from environments.yaml. Repeatable.
    /// Use `--preset list` to print available preset names.
    #[arg(long, value_name = "NAME")]
    pub preset: Vec<String>,

    /// Inject an arbitrary env var into the container (KEY=VALUE). Repeatable.
    #[arg(long, value_name = "KEY=VALUE")]
    pub env: Vec<String>,

    /// Force a rebuild of the agent image, ignoring the layer cache.
    /// The base image (apt deps) is always built with cache to keep rebuilds fast.
    #[arg(long)]
    pub no_cache: bool,

    /// Print the commands to be run instead of executing them.
    #[arg(long)]
    pub dry_run: bool,

    /// Pass VERBOSE=1 into the container environment.
    #[arg(long)]
    pub verbose: bool,

    /// Suppress image build output.
    #[arg(long)]
    pub quiet: bool,

    /// Set the LLM agent version to install (default: latest).
    /// Applies to --harness pi only.
    #[arg(long, short = 'v', value_name = "VERSION")]
    pub harness_version: Option<String>,

    /// Keep the container after it exits instead of removing it automatically.
    #[arg(long)]
    pub preserve_container: bool,

    /// Create a temporary directory with mktemp -d and use it as the container
    /// workdir. The directory persists after the container exits.
    /// Conflicts with --file and --scratchpad.
    #[arg(long, conflicts_with_all = ["file", "scratchpad"])]
    pub tmp: bool,

    /// Create (or reuse) ~/.local/share/orka/scratch/<NAME> and use it as the
    /// container workdir. Conflicts with --file and --tmp.
    #[arg(long, value_name = "NAME", conflicts_with_all = ["file", "tmp"])]
    pub scratchpad: Option<String>,

    /// Mount only specific files into the container instead of the entire working
    /// directory. Repeatable. Each file is mounted at the same absolute path it
    /// has on the host. The container workdir is set to the invoking directory.
    #[arg(long, short = 'f', value_name = "FILE", conflicts_with_all = ["tmp", "scratchpad"])]
    pub file: Vec<std::path::PathBuf>,

    /// Print the license text and exit.
    #[arg(long)]
    pub print_license: bool,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Configuration and shell integration helpers.
    Config {
        #[command(subcommand)]
        command: ConfigCommand,
    },
}

#[derive(Subcommand, Debug)]
pub enum ConfigCommand {
    /// Write default config files to ~/.config/orka/.
    /// Skips any file that already exists.
    Init,

    /// Print a shell completion script to stdout.
    ///
    /// bash:       orka config completions bash > ~/.local/share/bash-completion/completions/orka
    /// zsh:        orka config completions zsh  > "${fpath[1]}/_orka"
    /// fish:       orka config completions fish > ~/.config/fish/completions/orka.fish
    /// elvish:     orka config completions elvish
    /// powershell: orka config completions powershell
    #[command(verbatim_doc_comment)]
    Completions {
        /// Shell dialect to emit.
        #[arg(value_enum)]
        shell: Shell,
    },

    /// Print the paths orka reads configuration from.
    Path,
}
