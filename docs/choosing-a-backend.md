# Choosing a backend

orka supports four backends: Docker, Podman, Apple `container` (alpha), and bubblewrap. The first three are container engines: they build an OCI image containing the agent and run each session inside it. Bubblewrap is a different model — it sandboxes the host filesystem directly without building or caching any image.

Select a backend with `--engine`, or set `engine` in `~/.config/orka/config.yaml` to avoid repeating the flag.

## Docker

Docker is the default. If it is already installed and running, no extra configuration is needed.

The image layer cache means subsequent runs are fast: only the agent layer is rebuilt when the harness version changes. The base OS layer, which changes rarely, is cached separately.

Docker requires a background daemon (`dockerd`). On most Linux desktops and servers this is managed by systemd and always running.

orka does not restrict backends by platform: it invokes whatever `docker` resolves to on `PATH`, so Docker Desktop on macOS should work the same way. This combination is untested. Bind mounts pass through a virtual filesystem share rather than a native mount, so large trees are slower than on Linux.

## Podman

Podman is daemonless. Each invocation spawns a container process directly with no persistent background service. This makes it a better fit for headless servers, CI, or setups where running a daemon is undesirable.

The image format and build process are identical to Docker, so layer caching works the same way. The API surface is compatible enough that most Docker usage transfers directly.

```sh
orka --engine podman
```

## Apple container (alpha)

This backend is alpha. It is not covered by automated testing, and both orka's support for it and the `container` CLI itself are subject to change. Use Docker or Podman where either is available.

[Apple `container`](https://github.com/apple/container) runs Linux containers on macOS, each inside its own lightweight virtual machine. It requires macOS 26 or later on Apple silicon.

```sh
orka --engine container
```

Start the background services before the first run:

```sh
container system start
```

orka checks that they are running and reports the required command if they are not. The BuildKit builder used by `container build` starts on demand.

Two differences from Docker and Podman affect what orka emits:

- **No `--security-opt`.** The flag does not exist in the `container` CLI, so `no-new-privileges` is not applied. The container process still runs as your host UID and GID.
- **`--cap-drop` depends on the version.** `container` gained capability flags in 0.12.0. orka probes `container run --help` and passes `--cap-drop=ALL` only when it is accepted; older versions abort on unknown flags. Each container runs in its own VM either way.
- **Per-container VMs.** Startup costs more than a shared daemon, and bind mounts are virtiofs shares rather than direct host mounts.

Only arm64 images are built, matching the host architecture.

## Bubblewrap

Bubblewrap (`bwrap`) is a Linux-only user-namespace sandbox. It does not build or cache any image. Instead, it mounts a subset of the host filesystem — standard system paths, your agent config directories, the agent's own installation tree, and any volumes you specify — into an isolated namespace and runs the agent binary directly.

This has practical consequences:

- **No build step.** Sessions start immediately. There is no image to build or pull.
- **Agent binary must be installed on the host.** Install the agent before using this backend. For pi: `bun install -g @earendil-works/pi-coding-agent`.
- **The binary is located via PATH.** Installs outside the system directories work without configuration, including version managers and npm-style global installs under your home directory; orka mounts whatever tree the binary actually resolves to. Set `pi-path` (or `claude-path` / `codex-path`) in `~/.config/orka/config.yaml` only when the binary is not on PATH. See [user defaults](getting-started.md#user-defaults).
- **Linux only.** Bubblewrap is not available on macOS or other platforms. Use the alpha `--engine container` there.

```sh
orka --engine bubblewrap
```

Use bubblewrap when you want the lowest runtime overhead and are comfortable managing the agent installation yourself. It is also useful when a container engine is not available or not permitted in your environment.

The isolation model differs from OCI containers. Mounted paths are bind-mounted read-only; the namespace boundary is provided by Linux user namespaces rather than a container runtime. Network access inside the sandbox is unrestricted (agents need outbound internet access).
