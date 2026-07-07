# orka

Agent runtime container wrapper. A single Rust binary that builds Docker images
containing your choice of agent runtime (pi, claude-code, or codex) and drops
you into one, with your current directory mounted as a volume.

## Usage

```
orka [OPTIONS] [CONTAINER_ARGS]...
```

`CONTAINER_ARGS` are forwarded verbatim to the agent inside the container.

### Options

| Flag | Description |
|---|---|
| `--runtime <RUNTIME>` | Agent runtime to use: `pi` (default), `claude`, or `codex`. |
| `--preset <NAME>` | Load volumes and env vars from a named preset in `~/.config/orka/environments.yaml`. Use `--preset list` to print available presets. |
| `--env KEY=VALUE` | Inject an env var into the container. Repeatable. |
| `--no-cache` | Rebuild the agent image ignoring Docker's layer cache. The base image (apt deps) is always cached. |
| `--pi-version <VER>` | Install a specific `@earendil-works/pi-coding-agent` version instead of `latest`. Applies to `--runtime pi` only. |
| `--ephemeral` | Remove the container on exit (`docker run --rm`). |
| `--no-browser` | Skip installing agent-browser and Chromium. Applies to `--runtime pi` only. |
| `--no-extensions` / `-N` | Hide all auto-discovered pi extensions for this run. Applies to `--runtime pi` only. |
| `--quiet` / `-q` | Suppress Docker build output. |
| `--debug` | Pass `--debug` to Docker build and run. |
| `--dry-run` | Print the Docker commands without executing them. |

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
