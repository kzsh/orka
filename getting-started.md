# Getting started

## Prerequisites

A container engine must be installed and running. orka supports Docker (default), Podman, and nerdctl. Verify your engine is available:

```sh
docker info
# or: podman info / nerdctl info
```

To use Podman or nerdctl, pass `--engine podman` or `--engine nerdctl` on each invocation, or set `engine` in `~/.config/orka/config.yaml` (see [User defaults](#user-defaults) below).

orka builds and caches a container image on first run. Subsequent runs reuse the cached image, so the initial build is slower than later ones.

## API keys

All three runtimes read API keys from your host environment. orka passes the following variables into the container automatically if they are set:

| Variable | Used by |
|---|---|
| `ANTHROPIC_API_KEY` | `claude` runtime (required); `pi` runtime when running Anthropic models |
| `OPENAI_API_KEY` | `codex` runtime (required); `pi` runtime when running OpenAI models |
| `OPEN_ROUTER_KEY` | `pi` runtime when routing through OpenRouter |

Export the relevant key(s) in your shell profile (`~/.bashrc`, `~/.zshrc`, etc.):

```sh
export ANTHROPIC_API_KEY=sk-ant-...
export OPENAI_API_KEY=sk-...
export OPEN_ROUTER_KEY=sk-or-...
```

orka reads these from your environment at runtime — you do not need to write them into any config file.

## First run

From a project directory:

```sh
orka
```

This mounts the current directory into the container and starts the default runtime (`pi`). The first run downloads and builds the image; expect it to take a minute or two.

To use a different runtime:

```sh
orka --runtime claude
orka --runtime codex
```

## Preset configuration

Presets let you inject volumes and environment variables without repeating flags on every invocation. Copy the [environments.yaml](https://raw.githubusercontent.com/kzsh/orka/main/config/environments.yaml) template from the repository:

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

This step is optional. If you always want to use the same engine or runtime without typing the flag every time, create `~/.config/orka/config.yaml` from the [config.yaml](https://raw.githubusercontent.com/kzsh/orka/main/config/config.yaml) template:

```sh
mkdir -p ~/.config/orka
curl -Lo ~/.config/orka/config.yaml \
  https://raw.githubusercontent.com/kzsh/orka/main/config/config.yaml
```

Uncomment and set any of the supported keys: `engine`, `runtime`, `harness`, `no_browser`. The `harness` key pins the agent version installed in the image — useful when you want the environment to stay consistent across machines or after an update. Any flag supplied on the command line takes precedence over the config file.

## Shadow configuration

To keep credentials or sensitive files out of the agent's context on every project, copy the global shadow template:

```sh
curl -Lo ~/.config/orka/orkashadow \
  https://raw.githubusercontent.com/kzsh/orka/main/config/orkashadow
```

Uncomment the patterns that apply to your setup. Files matched by these patterns are replaced with empty read-only stubs inside the container. See [shadow files](shadow-files.md) for a full guide, or [how it works](how-it-works.md) for the mechanism.
