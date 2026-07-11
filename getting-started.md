# Getting started

## Prerequisites

Docker must be installed and the daemon must be running. Verify with:

```sh
docker info
```

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

Presets let you inject volumes and environment variables without repeating flags on every invocation. Copy the bundled template from the repository:

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
