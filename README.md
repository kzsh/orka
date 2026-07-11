# orka

**Latest:** 2026-07-11

orka runs AI coding agents inside Docker containers. Each session gets only the files and credentials you explicitly hand it — the agent cannot reach the rest of your filesystem or your shell environment unless you mount them in. This gives you contained, reproducible agent sessions without giving agents unrestricted access to your home directory.

Three runtimes are supported: [pi](https://pi.earendil.works), [claude-code](https://docs.anthropic.com/en/docs/claude-code), and [Codex](https://openai.com/index/openai-codex/). The container image is built on first run and cached for subsequent runs.

See [getting started](getting-started.md) to set up API keys and run your first session.

## Install

Download the binary for your platform from [Releases](../../releases), make it executable, and put it on your `PATH`:

```sh
curl -Lo orka https://github.com/kzsh/orka/releases/latest/download/orka-x86_64-unknown-linux-musl
chmod +x orka
mv orka ~/.local/bin/
```

| File | Platform |
|---|---|
| `orka-x86_64-unknown-linux-gnu` | Linux x86\_64 (glibc) |
| `orka-x86_64-unknown-linux-musl` | Linux x86\_64 (static) |
| `orka-aarch64-unknown-linux-gnu` | Linux ARM64 (glibc) |
| `orka-aarch64-unknown-linux-musl` | Linux ARM64 (static) |

Prefer the `musl` variant unless you have a specific reason not to — it has no runtime dependencies.

## Usage

Run orka from a project directory to mount it into the container and start the agent:

```sh
orka
```

Mount specific files instead of the entire directory:

```sh
orka -f src/main.rs -f Cargo.toml
```

Use a temporary directory as the workdir (persists after the container exits):

```sh
orka --tmp
```

Use a named scratch directory (created at `~/.local/share/orka/scratch/<NAME>` (`XDG_DATA_HOME`)):

```sh
orka --scratchpad my-task
```

## Options

| Flag | Description |
|---|---|
| `--runtime` | Agent runtime: `pi` (default), `claude`, `codex` |
| `--preset <NAME>` | Apply a named preset from `environments.yaml`. Repeatable. |
| `--env <KEY=VALUE>` | Inject an env var into the container. Repeatable. |
| `--file` / `-f <FILE>` | Mount a specific file instead of the CWD. Repeatable. |
| `--tmp` | Use a temporary directory as the container workdir. |
| `--scratchpad <NAME>` | Use a named persistent scratch directory as the workdir. |
| `--harness-version` / `-v` | Pin the agent version to install (pi only). |
| `--no-browser` | Skip installing agent-browser and Chromium (pi only). |
| `--ephemeral` | Remove the container on exit. |
| `--no-cache` | Rebuild the agent image without Docker layer cache. |
| `--dry-run` | Print Docker commands without running them. |
| `--quiet` / `-q` | Suppress build output. |

## Presets

Presets are named configurations defined in `~/.config/orka/environments.yaml`. Each preset can specify volumes to mount and environment variables to inject. Presets can be stacked with multiple `--preset` flags.

See [`config/environments.yaml`](config/environments.yaml) for the format.
