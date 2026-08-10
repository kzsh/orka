# orka

Orka runs LLM coding agents inside containers. Each session gets only the file-system context you choose to mount. This gives you agent sessions that don't have unrestricted access to your home directory.

Three agent harnesses are supported: [pi](https://pi.earendil.works), [claude-code](https://docs.anthropic.com/en/docs/claude-code), and [Codex](https://openai.com/index/openai-codex/). 

Container engine backends (Docker, Podman, and Apple `container` in alpha) build an OCI image on first run and cache it for subsequent runs. The bubblewrap backend skips the image build entirely and runs the agent binary directly on the host.

See [getting started](docs/getting-started.md), [what is orka](docs/what-is-orka.md), [how it works](docs/how-it-works.md), [choosing a backend](docs/choosing-a-backend.md), [writing a preset](docs/writing-a-preset.md), [shell completions](docs/shell-completions.md), [invocations](docs/invocations.md), or [supported platforms](docs/supported-platforms.md) for more details.

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

Or build from source; see [CONTRIBUTING.md](CONTRIBUTING.md).


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
orka tmp
```

Use a named scratch directory (created at `~/.local/share/orka/scratch/<NAME>` (`XDG_DATA_HOME`)):

```sh
orka scratchpad my-task
```

Omit the name to choose an existing scratchpad from an interactive fuzzy list:

```sh
orka scratchpad
```

## Options

| Flag | Description |
|---|---|
| `--engine` | Backend: `docker` (default), `podman`, `container` (alpha), `bubblewrap` |
| `--harness` | Agent harness: `pi` (default), `claude`, `codex` |
| `--preset <NAME>` | Apply a named preset from `environments.yaml`. Repeatable. |
| `--env <KEY=VALUE>` | Inject an env var into the container. Repeatable. |
| `--volume <PATH[:CONTAINER_PATH]>` | Mount an extra host path. A bare path is mounted at the same absolute path inside the container. Repeatable. |
| `--file` / `-f <FILE>` | Mount a specific file instead of the CWD. Repeatable. |
| `--harness-version` / `-v` | Pin the agent version to install (pi only). |
| `--preserve-container` | Keep the container after it exits instead of removing it automatically. |
| `--no-cache` | Rebuild the agent image without Docker layer cache. |
| `--quiet` | Suppress image build output. |
| `--dry-run` | Print commands without running them. |
| `--verbose` | Pass `VERBOSE=1` into the container environment. |
| `--print-license` | Print the license text and exit. |

Every flag above is global: it can be given before or after a subcommand, so `orka --preset gh scratchpad foo` and `orka scratchpad foo --preset gh` are equivalent. Each `--preset` takes exactly one name.

## Subcommands

| Command | Description |
|---|---|
| `orka config init` | Write default config files to `~/.config/orka/`. Skips any file that already exists. |
| `orka config path` | Print the paths orka reads configuration from. |
| `orka config completions <SHELL>` | Print a shell completion script for `bash`, `zsh`, `fish`, `elvish`, or `powershell`. |
| `orka scratchpad [NAME]` (alias `orka scratch`) | Use `~/.local/share/orka/scratch/<NAME>` as the workdir, creating it if needed. Without `NAME`, select an existing scratchpad interactively. |
| `orka scratchpad --list` | Print existing scratchpad names and exit. |
| `orka tmp` | Use a fresh `mktemp -d` directory as the workdir. It persists after the container exits. |

See [shell completions](docs/shell-completions.md) for per-shell installation paths.

## Presets

Presets are named configurations defined in `~/.config/orka/environments.yaml`. Each preset can specify volumes to mount and environment variables to inject. Presets can be stacked with multiple `--preset` flags.

Presets you want on every run can be listed under `preset` in `config.yaml` instead of being typed each time. Naming an always-on preset again on the command line is a no-op, not a duplicate mount.

See [`config/environments.yaml`](config/environments.yaml) for the format.

## User defaults

Persistent defaults can be set in `~/.config/orka/config.yaml`. Copy the bundled template to get started:

```sh
mkdir -p ~/.config/orka
curl -Lo ~/.config/orka/config.yaml \
  https://raw.githubusercontent.com/kzsh/orka/main/config/config.yaml
```

| Key | Effect |
|---|---|
| `engine` | Default backend: `docker`, `podman`, `container`, `bubblewrap`. |
| `harness` | Default harness: `pi`, `claude`, `codex`. |
| `harness-version` | Agent version to install (pi only). |
| `pi-path`, `claude-path`, `codex-path` | Absolute path to each agent binary. Used only by the bubblewrap backend. |
| `harness-args` | Extra arguments per harness, forwarded to the agent. |
| `preset` | Presets applied to every run, as if passed with `--preset`. |
| `env` | `KEY=VALUE` pairs injected into every run, as if passed with `--env`. |
| `volume` | Paths mounted on every run, as if passed with `--volume`. |
| `no-cache`, `verbose`, `quiet`, `preserve-container` | Set the corresponding flag on every run (`true` or `false`). |

A flag supplied on the command line takes precedence over the config file value. `preset`, `env`, and `volume` are additive: command-line values are appended to the configured ones. Boolean keys can only turn a flag on, since the flags have no negated form.

`harness-args` sets agent flags you would otherwise type after `--` on every run. They are placed ahead of any arguments you do pass after `--`, so a trailing prompt stays last:

```yaml
harness: claude
harness-args:
  claude:
    - --dangerously-skip-permissions
```

With that config, `orka -- "fix the build"` runs `claude --dangerously-skip-permissions 'fix the build'` inside the container.

## Custom base image

Place a `Dockerfile.base` in `~/.config/orka/` to override the base image layer. Orka reads it automatically when present and uses it in place of the embedded default. To revert to the default temporarily, rename or remove the file.

The default base installs system packages, agent-browser, and Chromium. It is built with cache and shared across harnesses, so changes here affect all of them. To run without browser support, omit the agent-browser install from your custom file.

See [custom base image](docs/custom-base-image.md) for requirements, examples, and the `AGENT_BROWSER_EXECUTABLE_PATH` convention.

## Global skills

Orka automatically mounts `~/.agents` into the container when that directory exists on the host. This follows the [Agent Skills standard](https://agentskills.io) convention: `~/.agents/skills/` is the harness-neutral location for skills you want available regardless of which agent you run. Pi discovers skills from that path in addition to `~/.pi/agent/skills/`. Other compliant harnesses do the same.

Without the mount, skills stored in `~/.agents/skills/` would be invisible inside the container. No configuration is required; the mount happens whenever the directory is present.

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

## License

MIT. See [LICENSE](LICENSE). Third-party components bundled in the binary are listed in [THIRD_PARTY_LICENSES](THIRD_PARTY_LICENSES); `orka --print-license` prints both.
