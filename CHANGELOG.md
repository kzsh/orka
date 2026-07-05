# Changelog

## Unreleased

- Support claude-code (`--runtime claude`)
- Support Codex (`--runtime codex`)
- `--preset` to mount named volume and env var sets; presets can be stacked
- `--no-browser` to skip installing agent-browser and Chromium (browser support on by default)
- `--no-extensions` to hide auto-discovered pi extensions for a run
- `--harness-version` to pin the agent version installed in the image
- `--ephemeral` to remove the container on exit
- `--dry-run` to print Docker commands without executing them
