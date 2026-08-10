# Getting started

## Prerequisites

orka supports four backends. The requirements depend on which one you use.

**Docker, Podman** — A container engine must be installed and running. Verify it is available:

```sh
docker info
# or: podman info
```

orka builds and caches a container image on first run. Subsequent runs reuse the cached image, so the initial build is slower.

Neither backend is restricted to Linux. If `docker` is on `PATH` and its daemon is reachable, `--engine docker` should work, including Docker Desktop on macOS. That combination is untested.

**Apple container (alpha)** — macOS 26 or later on Apple silicon, with [container](https://github.com/apple/container) installed. Start its background services once:

```sh
container system start
```

**Bubblewrap** — No container engine is needed. Bubblewrap (`bwrap`) must be installed (it is available in most Linux distribution package repositories). The agent binary must also be installed on the host before running orka. For pi:

```sh
bun install -g @earendil-works/pi-coding-agent
```

See [choosing a backend](choosing-a-backend.md) for a full comparison of all four options.

## API keys

All three harnesses read API keys from your host environment. orka passes the following variables into the container automatically if they are set:

| Variable | Used by |
|---|---|
| `ANTHROPIC_API_KEY` | `claude` harness (required); `pi` harness when running Anthropic models |
| `OPENAI_API_KEY` | `codex` harness (required); `pi` harness when running OpenAI models |
| `OPEN_ROUTER_KEY` | `pi` harness when routing through OpenRouter |

Export the relevant key(s) in your shell profile (`~/.bashrc`, `~/.zshrc`, etc.):

```sh
export ANTHROPIC_API_KEY=sk-ant-...
export OPENAI_API_KEY=sk-...
export OPEN_ROUTER_KEY=sk-or-...
```

orka reads these from your environment at runtime — you do not need to write them into any config file.

For any other credentials a model or provider requires, export them in your shell profile and pass them through with `--env`:

```sh
# in ~/.bashrc or ~/.zshrc
export MY_PROVIDER_KEY=sk-...
```

```sh
orka --env MY_PROVIDER_KEY=$MY_PROVIDER_KEY
```

## First run

From a project directory:

```sh
orka
```

This mounts the current directory into the container and starts the default harness (`pi`). The first run downloads and builds the image; expect it to take a minute or two.

To use a different harness:

```sh
orka --harness claude
orka --harness codex
```

Set `harness` in `~/.config/orka/config.yaml` to make a harness the default for every session.

## Preset configuration

Presets let you inject volumes and environment variables without repeating flags on every invocation. Run `orka config init` to write the bundled template files to `~/.config/orka/`, then edit `environments.yaml` to match the paths on your system:

```sh
orka config init
```

Alternatively, download only the environments template:

```sh
mkdir -p ~/.config/orka
curl -Lo ~/.config/orka/environments.yaml \
  https://raw.githubusercontent.com/kzsh/orka/main/config/environments.yaml
```

Edit the file to match the paths on your system. A preset for Rust, for example, mounts your cargo and rustup directories so the agent can build Rust projects without re-downloading the toolchain each time:

```yaml
environments:
  rust:
    volumes:
      - ~/.cargo/:~/.cargo/
      - ~/.rustup/:~/.rustup/
```

Apply one or more presets at run time:

```sh
orka --preset rust
orka --preset rust --preset uv
```

Presets needed in every session can be listed under `preset` in `config.yaml` (see [user defaults](#user-defaults)) rather than passed each time.

See [`config/environments.yaml`](../config/environments.yaml) for the full set of bundled presets and a description of the format.

For a step-by-step guide to writing your own preset, see [writing a preset](writing-a-preset.md). For an explanation of how orka builds and runs containers, see [how it works](how-it-works.md).

## User defaults

This step is optional. If you always want to use the same engine or harness without typing the flag every time, run `orka config init` (it writes all config templates at once and skips any that already exist) or download only `config.yaml`:

```sh
mkdir -p ~/.config/orka
curl -Lo ~/.config/orka/config.yaml \
  https://raw.githubusercontent.com/kzsh/orka/main/config/config.yaml
```

Uncomment and set any of the supported keys:

| Key | Description |
|---|---|
| `engine` | Backend to use: `docker`, `podman`, `container` (alpha), `bubblewrap` |
| `harness` | Agent harness: `pi`, `claude`, `codex` |
| `harness-version` | Harness version to install (pi only; pins to a specific release) |
| `pi-path` | Absolute path to the pi binary (bubblewrap backend only) |
| `claude-path` | Absolute path to the claude binary (bubblewrap backend only) |
| `codex-path` | Absolute path to the codex binary (bubblewrap backend only) |
| `harness-args` | Extra arguments per harness, forwarded to the agent |
| `preset` | Presets applied to every run (same as repeating `--preset`) |
| `env` | `KEY=VALUE` pairs injected into every run (same as `--env`) |
| `no-cache`, `verbose`, `quiet`, `preserve-container` | Set the corresponding flag on every run |

Any flag supplied on the command line takes precedence over the config file. `preset` and `env` are additive: command-line values are appended to the configured ones.

For agent flags you would otherwise type after `--` on every run, use `harness-args`:

```yaml
harness: claude
harness-args:
  claude:
    - --dangerously-skip-permissions
```

## Shadow configuration

To keep credentials or sensitive files out of the agent's context on every project, run `orka config init` to write the bundled template, or download just the shadow template:

```sh
mkdir -p ~/.config/orka
curl -Lo ~/.config/orka/orkashadow \
  https://raw.githubusercontent.com/kzsh/orka/main/config/orkashadow
```

Uncomment the patterns that apply to your setup. Files matched by these patterns are replaced with empty read-only stubs inside the container. See [shadow files](shadow-files.md) for a full guide, or [how it works](how-it-works.md) for the mechanism.

## Shell completions

Install a completion script for your shell:

```sh
mkdir -p ~/.local/share/bash-completion/completions
orka config completions bash > ~/.local/share/bash-completion/completions/orka
```

See [shell completions](shell-completions.md) for zsh, fish, elvish, and powershell.
