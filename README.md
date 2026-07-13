# orka

**Latest:** 2026-07-13

Orka runs LLM coding agents inside containers. Each session gets only the file-system context you choose to mount. This gives you agent sessions that don't have unrestricted access to your home directory.

Three agent harnesses are supported: [pi](https://pi.earendil.works), [claude-code](https://docs.anthropic.com/en/docs/claude-code), and [Codex](https://openai.com/index/openai-codex/). 

Container engine backends (Docker, Podman, nerdctl) build an OCI image on first run and cache it for subsequent runs. The bubblewrap backend skips the image build entirely and runs the agent binary directly on the host.

See [getting started](getting-started.md), [what is orka](what-is-orka.md), [how it works](how-it-works.md), [choosing a backend](choosing-a-backend.md), or [writing a preset](writing-a-preset.md) for more details.

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
| `--engine` | Backend: `docker` (default), `podman`, `nerdctl`, `bubblewrap` |
| `--harness` | Agent harness: `pi` (default), `claude`, `codex` |
| `--preset <NAME>` | Apply a named preset from `environments.yaml`. Repeatable. |
| `--env <KEY=VALUE>` | Inject an env var into the container. Repeatable. |
| `--file` / `-f <FILE>` | Mount a specific file instead of the CWD. Repeatable. |
| `--tmp` | Use a temporary directory as the container workdir. |
| `--scratchpad <NAME>` | Use a named persistent scratch directory as the workdir. |
| `--harness-version` / `-v` | Pin the agent version to install (pi only). |
| `--no-browser` | Skip installing agent-browser and Chromium (pi only). |
| `--preserve-container` | Keep the container after it exits instead of removing it automatically. |
| `--no-cache` | Rebuild the agent image without Docker layer cache. |
| `--dry-run` | Print commands without running them. |
| `--verbose` | Show Docker build output (suppressed by default). |
| `--print-license` | Print the license text and exit. |

## Presets

Presets are named configurations defined in `~/.config/orka/environments.yaml`. Each preset can specify volumes to mount and environment variables to inject. Presets can be stacked with multiple `--preset` flags.

See [`config/environments.yaml`](config/environments.yaml) for the format.

## User defaults

Persistent defaults can be set in `~/.config/orka/config.yaml`. Copy the bundled template to get started:

```sh
mkdir -p ~/.config/orka
curl -Lo ~/.config/orka/config.yaml \
  https://raw.githubusercontent.com/kzsh/orka/main/config/config.yaml
```

Supported keys: `engine`, `harness`, `harness-version`, `no_browser`, `pi-path`, `claude-path`, `codex-path`. The `*-path` keys set the absolute path to each agent binary and are used only by the bubblewrap backend. Any flag supplied on the command line takes precedence over the config file value.

## Shadowing sensitive files

Files matching patterns in an `orkashadow` file are replaced with empty read-only stubs inside the container. The agent can see the filename but cannot read or write the content.

**Global patterns** (`~/.config/orka/orkashadow`) apply to every mount. Copy the bundled template:

```sh
mkdir -p ~/.config/orka
curl -Lo ~/.config/orka/orkashadow \
  https://raw.githubusercontent.com/kzsh/orka/main/config/orkashadow
```

**Per-repo patterns** live in a `.orkashadow` file at the root of any directory you mount and apply only to that directory.

Both files use `.gitignore` syntax: glob patterns, `!` negations, `**` depth wildcards.
