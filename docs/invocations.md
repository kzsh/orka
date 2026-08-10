# Common invocations


| Command line | `~/.config/orka/config.yaml` |
|---|---|
| `orka` | |
| `orka -f src/main.rs -f Cargo.toml` | |
| `orka --preset rust` | `preset: [rust]` |
| `orka --preset jira --preset rust` | `preset: [jira, rust]` |
| `orka --env RUST_LOG=debug` | `env: [RUST_LOG=debug]` |
| `orka --volume ~/.some-extra-dir` | `volume: [~/.some-extra-dir]` |
| `orka --volume ~/data:/mnt/data` | `volume: [~/data:/mnt/data]` |
| `orka scratchpad research` | |
| `orka scratchpad` (interactive picker) | |
| `orka scratchpad --list` | |
| `orka tmp` | |
| `orka --engine podman` | `engine: podman` |
| `orka --engine container` (alpha) | `engine: container` |
| `orka --engine bubblewrap` | `engine: bubblewrap` |
| `orka --harness claude` | `harness: claude` |
| `orka --harness-version 1.2.3` | `harness-version: "1.2.3"` |
| `orka --harness claude -- --dangerously-skip-permissions` | `harness-args: {claude: [--dangerously-skip-permissions]}` |
| `orka --no-cache` | `no-cache: true` |
| `orka --verbose` | `verbose: true` |
| `orka --quiet` | `quiet: true` |
| `orka --preserve-container` | `preserve-container: true` |
| `orka config init` | |
| `orka config path` | |
| `orka config completions zsh` | |
