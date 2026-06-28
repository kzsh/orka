# pita

Pi in a container. A single Rust binary that builds a Docker image containing
[@earendil-works/pi-coding-agent](https://www.npmjs.com/package/@earendil-works/pi-coding-agent)
and drops you into it, with your current directory mounted as a volume.

## Usage

```
pita [OPTIONS] [CONTAINER_ARGS]...
```

`CONTAINER_ARGS` are forwarded verbatim to `pi` inside the container.

### Options

| Flag | Description |
|---|---|
| `--preset <NAME>` | Load volumes and env vars from a named preset in `~/.config/pita/environments.yaml`. Use `--preset list` to print available presets. |
| `--env KEY=VALUE` | Inject an env var into the container. Repeatable. |
| `--no-cache` | Rebuild the pi image ignoring Docker's layer cache. The base image (apt deps) is always cached. |
| `--pi-version <VER>` | Install a specific `@earendil-works/pi-coding-agent` version instead of `latest`. |
| `--ephemeral` | Remove the container on exit (`docker run --rm`). |
| `--quiet` / `-q` | Suppress Docker build output. |
| `--debug` | Pass `--debug` to Docker build and run. |
| `--dry-run` | Print the Docker commands without executing them. |

### Presets

Copy the bundled template and edit it to match your system:

```sh
mkdir -p ~/.config/pita
cp config/environments.yaml ~/.config/pita/environments.yaml
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
# binary at target/release/pita
```

The Dockerfiles and `entrypoint.sh` are embedded in the binary at compile time
via `include_str!`, so the released binary is fully self-contained.

## Running tests

```sh
cargo test
```
