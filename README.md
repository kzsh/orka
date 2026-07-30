# orka

Agent runtime container wrapper. A single Rust binary that builds container
images containing your choice of agent runtime (pi, claude-code, or codex) and
drops you into one, with your current directory mounted as a volume.

## Usage

```
orka [OPTIONS] [CONTAINER_ARGS]...
```

`CONTAINER_ARGS` are forwarded verbatim to the agent inside the container.

### Options

| Flag | Description |
|---|---|
| `--engine <ENGINE>` | Isolation backend to use: `docker` (default), `podman`, or `bubblewrap`. |
| `--runtime <RUNTIME>` | Agent runtime to use: `pi` (default), `claude`, or `codex`. |
| `--preset <NAME>` | Load volumes and env vars from a named preset in `~/.config/orka/environments.yaml`. Use `--preset list` to print available presets. |
| `--env KEY=VALUE` | Inject an env var into the container. Repeatable. |
| `--file` / `-f <FILE>` | Mount a specific file instead of the CWD. Repeatable. |
| `--tmp` | Use a temporary directory as the container workdir. |
| `--scratchpad <NAME>` | Use a named persistent scratch directory as the workdir. |
| `--harness-version` / `-v <VER>` | Install a specific agent harness version instead of `latest`. Applies to `--runtime pi` only. |
| `--no-browser` | Skip installing agent-browser and Chromium. Applies to `--runtime pi` only. |
| `--preserve-container` | Keep the container after it exits instead of removing it automatically. |
| `--no-cache` | Rebuild the agent image ignoring the layer cache. The base image (apt deps) is always cached. |
| `--dry-run` | Print the commands to be run instead of executing them. |
| `--verbose` | Show build output instead of suppressing it. |
| `--print-license` | Print the license text and exit. |

### Subcommands

| Command | Description |
|---|---|
| `orka config init` | Write the bundled config templates to `~/.config/orka/`, skipping existing files. |
| `orka config path` | Print the paths orka reads configuration from. |
| `orka config completions <SHELL>` | Print a completion script for `bash`, `zsh`, `fish`, `elvish`, or `powershell`. |

Installing completions:

```sh
# bash
mkdir -p ~/.local/share/bash-completion/completions
orka config completions bash > ~/.local/share/bash-completion/completions/orka

# zsh (any directory on $fpath)
orka config completions zsh > "${fpath[1]}/_orka"

# fish
mkdir -p ~/.config/fish/completions
orka config completions fish > ~/.config/fish/completions/orka.fish
```

### Configuration file

Copy the bundled template to set persistent defaults:

```sh
mkdir -p ~/.config/orka
cp config/config.yaml ~/.config/orka/config.yaml
```

`config.yaml` supports: `engine`, `runtime`, `harness`, `no_browser`. Any flag
supplied on the command line takes precedence over the config file value.

### Presets

Copy the bundled template and edit it to match your system:

```sh
mkdir -p ~/.config/orka
cp config/environments.yaml ~/.config/orka/environments.yaml
```

Preset YAML format:

```yaml
environments:
  rust:
    volumes:
      - ~/.cargo/:~/.cargo/
      - ~/.rustup/:~/.rustup/
  go:
    volumes:
      - /usr/local/go:/usr/local/go
      - ~/go:~/go
    env:
      - PATH=/usr/local/go/bin:~/go/bin:$PATH
```

Leading `~` and `$VAR` / `${VAR}` references in env values are expanded from
the host environment at runtime.

### Shadowing sensitive files

Files matching patterns in an `orkashadow` file are replaced with empty
read-only stubs inside the container. The agent can see the filename but
cannot read or write the content.

**Global patterns** apply to every mount:

```sh
cp config/orkashadow ~/.config/orka/orkashadow
```

**Per-repo patterns** apply only to the directory they live in. Place a
`.orkashadow` file at the root of any directory you mount. The syntax is
identical to `.gitignore`: glob patterns, `!` negations, `**` depth
wildcards.

## How it works

When you run `orka`, it builds two Docker images in sequence: a base image
containing slow-changing apt dependencies, then the agent image on top. The
agent image embeds the chosen runtime (pi, claude-code, or codex). Your working
directory (or the path specified via `--file`, `--tmp`, or `--scratchpad`) is
mounted into the container, and `orka` drops you into an interactive session.

Both the Dockerfiles and the entrypoint script are embedded in the binary at
compile time, so no external files are needed at runtime.

To see the exact `docker build` and `docker run` commands that would be issued
without running them, pass `--dry-run`. This is useful for understanding the
full set of volume mounts, environment variables, and flags in effect for a
given invocation.

## Building

Rust stable required.

```sh
cargo build --release
# binary at target/release/orka
```

The Dockerfiles and entrypoint scripts are embedded in the binary at compile
time via `include_str!`, so the released binary is fully self-contained.

## Running tests

```sh
cargo test
```
