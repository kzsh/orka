# orka

**Latest:** 2026-07-11

Orka runs LLM coding agents inside containers. Each session gets only the file-system context you choose to mount. This gives you agent sessions that don't have unrestricted access to your home directory.

Three runtimes are supported: [pi](https://pi.earendil.works), [claude-code](https://docs.anthropic.com/en/docs/claude-code), and [Codex](https://openai.com/index/openai-codex/). 

The container image is built on each run first run and cached for subsequent runs. 

See [getting started](getting-started.md), [what is orka](what-is-orka.md), or [how it works](how-it-works.md) for more details.

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


## Usage

Run `orka` from a project directory to mount it into the container and start the agent:

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
| `--preserve-container` | Keep the container after it exits instead of removing it automatically. |
| `--no-cache` | Rebuild the agent image without Docker layer cache. |
| `--dry-run` | Print Docker commands without running them. |
| `--verbose` | Show Docker build output (suppressed by default). |
| `--print-license` | Print the license text and exit. |

## Presets

Presets are named configurations defined in `~/.config/orka/environments.yaml`. Each preset can specify volumes to mount and environment variables to inject. Presets can be stacked with multiple `--preset` flags.

See [`config/environments.yaml`](config/environments.yaml) for the format.
