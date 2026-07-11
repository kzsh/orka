# How it works

## Container isolation

orka runs the agent inside a Docker container. The agent process has no access to the host filesystem beyond what you explicitly mount in. API keys and other environment variables are passed in selectively rather than inherited from the host shell environment wholesale.

## Image building

orka builds the agent image every time it runs. In practice this is fast because Docker's layer cache means only changed layers are rebuilt. The base layer — which installs system packages — is kept separate and rarely changes. The agent layer on top of it, which installs the agent runtime itself, is what gets rebuilt when you update orka or pin a new agent version with `--harness-version`.

A full rebuild from scratch (bypassing the cache) can be forced with `--no-cache`.

## User and permission mirroring

orka reads your user ID, group ID, and username from the host at runtime and passes them as build arguments to the image. The container creates a user with the same UID, GID, and name before starting the agent.

This means:

- Files written inside the container are owned by your host user, not root. No ownership mismatch on mounted directories.
- Paths that include your home directory (e.g. from presets) resolve correctly inside the container because the username matches.
- The agent sees a familiar environment rather than a generic root shell.
