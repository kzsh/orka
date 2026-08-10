# How it works

> Orka is a thin CLI. It shells out to whichever container engine you have installed — it does not speak the Docker API directly.

## Isolation model

orka runs the agent inside a container. The agent process has no access to the
host filesystem beyond what you explicitly mount in. API keys and other
environment variables are passed in selectively rather than inherited from the
host shell environment wholesale. All Linux capabilities are dropped at
container start (`--cap-drop=ALL`); the agent cannot acquire elevated privileges.
Apple's `container` only accepts that flag from 0.12.0 onwards, so orka probes
for it and omits it on older versions, where each container has its own VM.

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

**Container engines** (Docker, Podman, and Apple `container` in alpha) build an OCI image
for the agent harness and run each session inside a container. orka shells out
to the engine binary — it runs `docker build`, `docker run`, and so on as
subprocesses. Behaviour therefore matches whatever version of the engine is
installed on the host. Docker is the default; `--engine podman` and
`--engine container` select alternatives. Pass `--dry-run` to see the exact
commands that would be issued.

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

This section applies to container engine backends (Docker, Podman).
The bubblewrap backend does not build or cache any image.

orka builds the agent image on every invocation. The base layer — which
installs system packages, agent-browser, and Chromium — changes rarely and is
kept as a separate cached image (`orka-base`). The agent layer on top, which
installs the agent harness itself, is what gets rebuilt when orka is updated
or a new harness version is pinned with `--harness-version`. A full cache
bypass is available with `--no-cache`.

To replace the base layer with a custom one — for example to use a different
distribution or strip agent-browser — place a `Dockerfile.base` in
`~/.config/orka/`. See [custom base image](custom-base-image.md) for details.

## User and permission mirroring

For container engine backends, orka reads the invoking user's UID, GID, and
username from the host and passes them as build arguments. The container image
creates a matching user before starting the agent. Files written inside the
container are therefore owned by the host user, not root, and paths that
include the home directory resolve correctly because the username matches.

At run time the two engines reach that result differently. Docker is given
`--user uid:gid`. Podman is given `--userns=keep-id` instead, which maps the
host user into the container at the same UID on its own. The two are not
combined: `--user` would make Podman look the numeric UID back up as a name,
which fails outright for users who live in LDAP or sssd rather than
`/etc/passwd`.

For the bubblewrap backend, no user mapping is needed. The agent process runs
as the invoking user directly.

## Inspecting a run

Pass `--dry-run` to print the exact build and run commands that would be issued without executing them. Because orka shells out to the engine binary, the output is a literal sequence of commands you can copy and run yourself.
