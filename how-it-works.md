# How it works

> Orka is a thin set of wrapping scripts and container files, written in Rust for portability.  
> Around 3,000 lines across 7 source files.

## Isolation model

orka runs the agent inside a container. The agent process has no access to the
host filesystem beyond what you explicitly mount in. API keys and other
environment variables are passed in selectively rather than inherited from the
host shell environment wholesale. All Linux capabilities are dropped at
container start (`--cap-drop=ALL`); the agent cannot acquire elevated privileges.

## Shadow mounts

When a mounted directory contains a `.orkashadow` file, or when a global
`~/.config/orka/orkashadow` file exists, orka identifies every file matched
by those patterns and mounts a zero-byte read-only file over each one inside
the container. The matched path is still visible to the agent but its content
is inaccessible and writes are refused. This keeps credentials, proprietary
logic, or other sensitive material out of the agent's context without
excluding the surrounding directory from the mount.

Both files use `.gitignore` syntax. Global patterns apply to every mount;
per-repo patterns apply only to the directory they accompany. Per-repo
patterns are evaluated after global ones and can negate global matches with
`!`. See [shadow files](shadow-files.md) for syntax reference and setup.

## Backend

orka supports two classes of backend.

**Container engines** (Docker, Podman, nerdctl) build an OCI image for the
agent harness and run each session inside a container. The engine binary is
invoked directly, so behaviour matches whatever version is installed on the
host. Docker is the default; `--engine podman` or `--engine nerdctl` selects
an alternative.

**Bubblewrap** (`--engine bubblewrap`) is a Linux-only user-namespace sandbox.
It does not build or cache any image. Instead, it bind-mounts a subset of the
host filesystem into a new namespace and runs the agent binary directly. The
agent binary must already be installed on the host. See
[choosing a backend](choosing-a-backend.md) for a full comparison.

## Agent harnesses

Three agent harnesses are supported: pi, claude-code, and codex. Each harness
has its own Dockerfile and produces a separate image. Images are tagged and
cached independently, so switching harnesses does not invalidate the cache for
others. The harness is selected per invocation with `--harness`.

## Image building

This section applies to container engine backends (Docker, Podman, nerdctl).
The bubblewrap backend does not build or cache any image.

orka builds the agent image on every invocation. The base layer — which
installs system packages and (for pi) Chromium — changes rarely and is kept
as a separate cached image. The agent layer on top, which installs the agent
harness itself, is what gets rebuilt when orka is updated or a new harness
version is pinned with `--harness-version`. A full cache bypass is available
with `--no-cache`.

## User and permission mirroring

For container engine backends, orka reads the invoking user's UID, GID, and
username from the host and passes them as build arguments. The container image
creates a matching user before starting the agent. Files written inside the
container are therefore owned by the host user, not root, and paths that
include the home directory resolve correctly because the username matches.

For the bubblewrap backend, no user mapping is needed. The agent process runs
as the invoking user directly.

## Inspecting a run

If you want to get a sense of how `orka` is building and running containers, try passing `--dry-run` to print the exact build and run commands that would be issued without executing them.
