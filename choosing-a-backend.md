# Choosing a backend

orka supports four backends: Docker, Podman, nerdctl, and bubblewrap. Docker, Podman, and nerdctl are container engines: they build an OCI image containing the agent and run each session inside it. Bubblewrap is a different model — it sandboxes the host filesystem directly without building or caching any image.

Select a backend with `--engine`, or set `engine` in `~/.config/orka/config.yaml` to avoid repeating the flag.

## Docker

Docker is the default. If it is already installed and running, no extra configuration is needed.

The image layer cache means subsequent runs are fast: only the agent layer is rebuilt when the harness version changes. The base OS layer, which changes rarely, is cached separately.

Docker requires a background daemon (`dockerd`). On most Linux desktops and servers this is managed by systemd and always running.

## Podman

Podman is daemonless. Each invocation spawns a container process directly with no persistent background service. This makes it a better fit for headless servers, CI, or setups where running a daemon is undesirable.

The image format and build process are identical to Docker, so layer caching works the same way. The API surface is compatible enough that most Docker usage transfers directly.

```sh
orka --engine podman
```

## nerdctl

nerdctl drives containerd directly. It is the natural choice if containerd is already present — for instance through Rancher Desktop, Lima, or a Kubernetes node setup — and you want to avoid running a separate Docker daemon.

Layer caching works the same way as Docker.

```sh
orka --engine nerdctl
```

## Bubblewrap

Bubblewrap (`bwrap`) is a Linux-only user-namespace sandbox. It does not build or cache any image. Instead, it mounts a subset of the host filesystem — standard system paths, your agent config directories, and any volumes you specify — into an isolated namespace and runs the agent binary directly.

This has practical consequences:

- **No build step.** Sessions start immediately. There is no image to build or pull.
- **Agent binary must be installed on the host.** Install the agent before using this backend. For pi: `bun install -g @earendil-works/pi-coding-agent`.
- **Non-standard install paths require configuration.** If the agent binary is not under `/usr`, `/opt`, `/bin`, or `/sbin`, set `pi-path` (or `claude-path` / `codex-path`) in `~/.config/orka/config.yaml` to the absolute path of the binary. See [user defaults](getting-started.md#user-defaults).
- **Linux only.** Bubblewrap is not available on macOS or other platforms.

```sh
orka --engine bubblewrap
```

Use bubblewrap when you want the lowest runtime overhead and are comfortable managing the agent installation yourself. It is also useful when a container engine is not available or not permitted in your environment.

The isolation model differs from OCI containers. Mounted paths are bind-mounted read-only; the namespace boundary is provided by Linux user namespaces rather than a container runtime. Network access inside the sandbox is unrestricted (agents need outbound internet access).
