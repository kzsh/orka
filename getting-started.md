# Getting started

## Prerequisites

orka supports four backends. The requirements depend on which one you use.

**Docker, Podman, nerdctl** — A container engine must be installed and running. Verify it is available:

```sh
docker info
# or: podman info / nerdctl info
```

orka builds and caches a container image on first run. Subsequent runs reuse the cached image, so the initial build is slower.

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

Presets let you inject volumes and environment variables without repeating flags on every invocation. Run `orka --init` to write the bundled template files to `~/.config/orka/`, then edit `environments.yaml` to match the paths on your system:

```sh
orka --init
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

See [`config/environments.yaml`](config/environments.yaml) for the full set of bundled presets and a description of the format.

For a step-by-step guide to writing your own preset, see [writing a preset](writing-a-preset.md). For an explanation of how orka builds and runs containers, see [how it works](how-it-works.md).

## User defaults

This step is optional. If you always want to use the same engine or harness without typing the flag every time, run `orka --init` (it writes all config templates at once and skips any that already exist) or download only `config.yaml`:

```sh
mkdir -p ~/.config/orka
curl -Lo ~/.config/orka/config.yaml \
  https://raw.githubusercontent.com/kzsh/orka/main/config/config.yaml
```

Uncomment and set any of the supported keys:

| Key | Description |
|---|---|
| `engine` | Backend to use: `docker`, `podman`, `nerdctl`, `bubblewrap` |
| `harness` | Agent harness: `pi`, `claude`, `codex` |
| `harness-version` | Harness version to install (pi only; pins to a specific release) |
| `no_browser` | Skip agent-browser and Chromium (pi only) |
| `pi-path` | Absolute path to the pi binary (bubblewrap backend only) |
| `claude-path` | Absolute path to the claude binary (bubblewrap backend only) |
| `codex-path` | Absolute path to the codex binary (bubblewrap backend only) |

Any flag supplied on the command line takes precedence over the config file.

## Shadow configuration

To keep credentials or sensitive files out of the agent's context on every project, run `orka --init` to write the bundled template, or download just the shadow template:

```sh
mkdir -p ~/.config/orka
curl -Lo ~/.config/orka/orkashadow \
  https://raw.githubusercontent.com/kzsh/orka/main/config/orkashadow
```

Uncomment the patterns that apply to your setup. Files matched by these patterns are replaced with empty read-only stubs inside the container. See [shadow files](shadow-files.md) for a full guide, or [how it works](how-it-works.md) for the mechanism.
