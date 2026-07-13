# Changelog

## latest

- `--init` to write the bundled config templates (`config.yaml`, `environments.yaml`, `orkashadow`) to `~/.config/orka/`. Files that already exist are skipped.
- `--quiet` to suppress image build output (build output is now shown by default; use `--quiet` to hide it).
- `--verbose` now passes `VERBOSE=1` into the container environment instead of controlling build output visibility.
- `--no-custom-dockerfile` to ignore a user-supplied `~/.config/orka/Dockerfile.base` and use the embedded default.
- Podman backend: `--userns=keep-id` is now passed automatically so container file ownership matches the host user.
- `--engine bubblewrap` to run the agent in a bubblewrap user-namespace sandbox instead of a container. No image is built; the agent binary must already be installed on the host. Bind-mounts system paths, agent config directories, and user-specified volumes into the namespace. Linux only.
- `pi-path`, `claude-path`, `codex-path` in `~/.config/orka/config.yaml` to set explicit paths to agent binaries when they are not in a standard location. Used only by the bubblewrap backend.
- `--engine` to select the container engine: `docker` (default), `podman`, or `nerdctl`. The engine binary is used for all build and run commands.
- `~/.config/orka/config.yaml` for persistent user defaults. Supports `engine`, `runtime`, `harness`, and `no_browser`. Command-line flags always win. Copy the bundled [`config/config.yaml`](config/config.yaml) to get started.
- `orkashadow` files to hide sensitive files from the agent. Files matching patterns in `~/.config/orka/orkashadow` (global) or `.orkashadow` (per-repo, placed at the root of any mounted directory) are replaced with empty read-only stubs inside the container. The agent can see the filename but cannot read or write the content. Uses `.gitignore` syntax. Copy the bundled [`config/orkashadow`](config/orkashadow) for annotated examples.
- `--file` / `-f` to mount specific files into the container rather than the entire working directory. Repeatable. Each file is mounted at its host path; the container workdir is set to the invoking directory.
- `--tmp` to create a temporary directory with `mktemp -d` and use it as the container workdir. The directory persists after the container exits.
- `--scratchpad <NAME>` to create or reuse `~/.local/share/orka/scratch/<NAME>` as the container workdir.
- Browser support is now built into a separate cached base image (`orka-browser-base`), avoiding a full Chromium reinstall on every image rebuild.
- Support claude-code (`--runtime claude`)
- Support Codex (`--runtime codex`)
- `--preset` to mount named volume and env var sets; presets can be stacked
- `--no-browser` to skip installing agent-browser and Chromium (browser support on by default)
- `--harness-version` to pin the agent version installed in the image
- `--preserve-container` to keep the container after it exits (containers are removed on exit by default)
- `--dry-run` to print commands without executing them
