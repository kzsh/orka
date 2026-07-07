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
#[command(name = "pita", about = "Pi (or Claude) in a container", version)]
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

    /// Set the @earendil-works/pi-coding-agent version to install (default: latest).
    /// Applies to --runtime pi only.
    #[arg(long, short = 'v', value_name = "VERSION")]
    pub pi_version: Option<String>,

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

    /// Mount an empty tmpfs over ~/.pi/agent/extensions inside the container,
    /// hiding all auto-discovered extensions for this run.
    /// Applies to --runtime pi only.
    #[arg(long, short = 'N')]
    pub no_extensions: bool,

    /// Arguments forwarded verbatim to the container (passed to the agent).
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    pub container_args: Vec<String>,
}
