# How it works

## Isolation model

orka runs the agent inside a container. The agent process has no access to the
host filesystem beyond what you explicitly mount in. API keys and other
environment variables are passed in selectively rather than inherited from the
host shell environment wholesale. All Linux capabilities are dropped at
container start; the agent cannot acquire elevated privileges.

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
`!`.

## Container engine

orka delegates all build and run operations to a container engine binary.
Docker is the default; `--engine podman` or `--engine nerdctl` selects an
alternative. The engine binary is invoked directly, so behaviour matches
whatever version is installed on the host.

## Agent runtimes

Three agent runtimes are supported: pi, claude-code, and codex. Each runtime
has its own Dockerfile and produces a separate image. Images are tagged and
cached independently, so switching runtimes does not invalidate the cache for
others. The runtime is selected per invocation with `--runtime`.

## Image building

orka builds the agent image on every invocation. The base layer — which
installs system packages and (for pi) Chromium — changes rarely and is kept
as a separate cached image. The agent layer on top, which installs the agent
runtime itself, is what gets rebuilt when orka is updated or a new harness
version is pinned with `--harness-version`. A full cache bypass is available
with `--no-cache`.

## User and permission mirroring

orka reads the invoking user's UID, GID, and username from the host and
passes them as build arguments. The container image creates a matching user
before starting the agent. Files written inside the container are therefore
owned by the host user, not root, and paths that include the home directory
resolve correctly because the username matches.
