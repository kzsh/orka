# Changelog

## latest

- `--file` / `-f` to mount specific files into the container rather than the entire working directory. Repeatable. Each file is mounted at its host path; the container workdir is set to the invoking directory.
- `--tmp` to create a temporary directory with `mktemp -d` and use it as the container workdir. The directory persists after the container exits.
- `--scratchpad <NAME>` to create or reuse `~/.local/share/orka/scratch/<NAME>` as the container workdir.
- Browser support is now built into a separate cached base image (`orka-browser-base`), avoiding a full Chromium reinstall on every image rebuild.
- Support claude-code (`--runtime claude`)
- Support Codex (`--runtime codex`)
- `--preset` to mount named volume and env var sets; presets can be stacked
- `--no-browser` to skip installing agent-browser and Chromium (browser support on by default)
- `--harness-version` to pin the agent version installed in the image
- `--ephemeral` to remove the container on exit
- `--dry-run` to print Docker commands without executing them
