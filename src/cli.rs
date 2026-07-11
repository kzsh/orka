use clap::{Parser, ValueEnum};

/// Which agent runtime to launch inside the container.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum Runtime {
    /// pi coding agent (default)
    #[default]
    Pi,
    /// Anthropic claude-code
    Claude,
    /// OpenAI codex
    Codex,
}

#[derive(Parser, Debug)]
#[command(name = "orka", about = "Agent runtime container wrapper", version)]
pub struct Cli {
    /// Agent runtime to use inside the container.
    #[arg(long, value_enum, default_value = "pi")]
    pub runtime: Runtime,

    /// Select a named preset from environments.yaml. Repeatable.
    /// Use --preset list to print available preset names.
    #[arg(long, value_name = "NAME")]
    pub preset: Vec<String>,

    /// Inject an arbitrary env var into the container (KEY=VALUE). Repeatable.
    #[arg(long, value_name = "KEY=VALUE")]
    pub env: Vec<String>,

    /// Force a rebuild of the agent image, ignoring Docker's layer cache.
    /// The base image (apt deps) is always built with cache to keep rebuilds fast.
    #[arg(long)]
    pub no_cache: bool,

    /// Print the Docker commands to be run instead of executing them.
    #[arg(long)]
    pub dry_run: bool,

    /// Run Docker build with minimal output.
    #[arg(long, short = 'q')]
    pub quiet: bool,

    /// Set the LLM agent version to install (default: latest).
    /// Applies to --runtime pi only.
    #[arg(long, short = 'v', value_name = "VERSION")]
    pub harness_version: Option<String>,

    /// Enable Docker debug mode on build and run.
    #[arg(long)]
    pub debug: bool,

    /// Remove the container automatically after it exits (docker run --rm).
    #[arg(long)]
    pub ephemeral: bool,

    /// Skip installing the agent-browser extension and Chromium (browser support is on by default).
    /// Applies to --runtime pi only.
    #[arg(long)]
    pub no_browser: bool,

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

    /// Arguments forwarded verbatim to the container (passed to the agent).
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    pub container_args: Vec<String>,
}
