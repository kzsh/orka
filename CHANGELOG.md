# Changelog

## latest

- `THIRD_PARTY_LICENSES` regenerated. It was missing `clap_complete`, `serde_ignored`, `itoa`, and `ryu`, all of which are linked into the released binaries. The license texts printed by `--print-license` are no longer HTML-escaped, and the summary at the top lists each license with the number of crates using it instead of an empty line per license.
- `orka scratch` is an alias for `orka scratchpad`.
- Top-level flags are now global and accepted in any position, before or after a subcommand. `orka scratchpad foo --preset gh --dry-run` previously failed with `unexpected argument '--preset' found`; it now behaves the same as `orka --preset gh --dry-run scratchpad foo`. `--preset list` is likewise handled regardless of position.

- orka is now open source under the MIT license (`LICENSE`), replacing the previous binary-only terms. `orka --print-license` prints the MIT text followed by `THIRD_PARTY_LICENSES`. The full source, documentation, and config templates live in one repository; topic pages moved under `docs/`.

- `--volume <PATH>` mounts an extra host path into the container, repeatable. A bare path is mounted at the same absolute path it has on the host; `HOST:CONTAINER` sets the destination explicitly. Paths already mounted (the working directory, or a preset volume) are skipped rather than mounted twice. `config.yaml` accepts a matching `volume` list applied to every run.
- `config.yaml` accepts `harness-args`: extra arguments per harness (`pi`, `claude`, `codex`), forwarded to the agent. They are placed ahead of anything passed after `--`, so a trailing prompt stays last. Removes the need to type flags such as `--dangerously-skip-permissions` on every run.
- `config.yaml` accepts `preset` and `env` lists, applied to every run as if passed with `--preset` and `--env`. Command-line values are appended to the configured ones. Presets are deduplicated, so naming an always-on preset again on the command line no longer produces a duplicate mount, which the container engines reject.
- `config.yaml` accepts `no-cache`, `verbose`, `quiet`, and `preserve-container` booleans, each setting the corresponding flag on every run. They can only turn a flag on; the flags have no negated form.
- A preset named in `config.yaml` but missing from `environments.yaml` now reports `config.yaml` and the offending name, instead of an error that reads as a mistyped `--preset` flag.

- `--tmp` replaced by the `orka tmp` subcommand. Same behaviour: the workdir is a fresh `mktemp -d` directory that persists after the container exits.
- `--scratchpad <NAME>` replaced by the `orka scratchpad [NAME]` subcommand. Without a name, existing scratchpads are shown in an interactive fuzzy selector (type to narrow, arrows or Ctrl-N/Ctrl-P to move, Enter to select, Esc to abort). `orka scratchpad --list` prints the names without starting a container. `orka config path` now also prints the scratchpad root.

- Podman backend: `--user` is no longer passed. It was redundant with `--userns=keep-id`, and it made Podman reverse-resolve the numeric UID to a username, which fails for LDAP/sssd users who are absent from `/etc/passwd`. Those runs failed with `unknown user error looking up user` and exit status 125.
- Bubblewrap backend: the agent binary's package tree is now bind-mounted, not just its `bin` directory. npm-style installs place a relative symlink in `bin` pointing into `lib/node_modules/...`; binding only `bin` left that symlink dangling and the sandbox failed with `execvp ...: No such file or directory`. The interpreter named in a script's shebang is now mounted too.
- `harness-version`, `pi-path`, `claude-path`, and `codex-path` in `config.yaml` are now honoured. They were read as snake_case while the documented and shipped format is kebab-case, so all four were silently ignored.
- agent-browser and Chromium are now bundled directly into the base image (`orka-base`). The separate `orka-browser-base` intermediate image is gone. The base builds once and is shared across harness rebuilds as before.
- Chrome is now reachable by the container's runtime user. `agent-browser install` ran as root during the base build and left the download in `/root/.agent-browser/browsers`, so browser tool calls failed in the running container. The download is relocated to `/opt/browser-cache` and exposed through `AGENT_BROWSER_EXECUTABLE_PATH`. The unused `PLAYWRIGHT_BROWSERS_PATH` variable is gone; custom base images that set it should set `AGENT_BROWSER_EXECUTABLE_PATH` instead.
- Documented that the Docker backend is not restricted to Linux. orka gates no backend by platform, so `--engine docker` works wherever `docker` resolves on `PATH`, Docker Desktop on macOS included. Untested, and noted as such.
- `--cap-drop=ALL` is now passed to Apple's `container` only when that binary accepts it. Capability flags were added in `container` 0.12.0, and earlier versions reject unknown options outright, so every run failed with `Error: Unknown option '--cap-drop'` and exit status 64. Docker and Podman are unaffected and are not probed.
- ARM64 base images now install the Chromium build published by Playwright instead of Chrome for Testing, which has no Linux ARM64 release. The base image build previously failed on Apple silicon at the `agent-browser install` step. `AGENT_BROWSER_EXECUTABLE_PATH` still points at `/opt/browser-cache/chrome` on both architectures. The revision is pinned by the `PLAYWRIGHT_CHROMIUM_REVISION` build argument.
- Architecture-dependent download steps in the bundled Dockerfiles now read BuildKit's `TARGETARCH` rather than `uname -m`, falling back to `dpkg --print-architecture` on the legacy builder. Cross-builds with `--platform` previously selected binaries for the builder's architecture instead of the image's.
- `--init` replaced by `orka config init`. New sibling subcommands: `orka config completions <SHELL>` prints a shell completion script (bash, zsh, fish, elvish, powershell), and `orka config path` prints the configuration paths orka reads.
- `--no-browser` removed. To run without browser support, provide a custom `~/.config/orka/Dockerfile.base` that omits agent-browser.

## previous

- `--init` to write the bundled config templates (`config.yaml`, `environments.yaml`, `orkashadow`) to `~/.config/orka/`. Files that already exist are skipped.
- `--quiet` to suppress image build output (build output is now shown by default; use `--quiet` to hide it).
- `--verbose` now passes `VERBOSE=1` into the container environment instead of controlling build output visibility.
- Podman backend: `--userns=keep-id` is now passed automatically so container file ownership matches the host user.
- `--engine bubblewrap` to run the agent in a bubblewrap user-namespace sandbox instead of a container. No image is built; the agent binary must already be installed on the host. Bind-mounts system paths, agent config directories, and user-specified volumes into the namespace. Linux only.
- `pi-path`, `claude-path`, `codex-path` in `~/.config/orka/config.yaml` to set explicit paths to agent binaries that are not on PATH. Used only by the bubblewrap backend.
- `--engine` to select the container engine: `docker` (default) or `podman`. The engine binary is used for all build and run commands.
- `~/.config/orka/config.yaml` for persistent user defaults. Supports `engine` and `harness`. Command-line flags always win. Copy the bundled [`config/config.yaml`](config/config.yaml) to get started.
- `orkashadow` files to hide sensitive files from the agent. Files matching patterns in `~/.config/orka/orkashadow` (global) or `.orkashadow` (per-repo, placed at the root of any mounted directory) are replaced with empty read-only stubs inside the container. The agent can see the filename but cannot read or write the content. Uses `.gitignore` syntax. Copy the bundled [`config/orkashadow`](config/orkashadow) for annotated examples.
- `--file` / `-f` to mount specific files into the container rather than the entire working directory. Repeatable. Each file is mounted at its host path; the container workdir is set to the invoking directory.
- `--tmp` to create a temporary directory with `mktemp -d` and use it as the container workdir. The directory persists after the container exits.
- `--scratchpad <NAME>` to create or reuse `~/.local/share/orka/scratch/<NAME>` as the container workdir.
- Support claude-code (`--harness claude`)
- Support Codex (`--harness codex`)
- `--preset` to mount named volume and env var sets; presets can be stacked
- `--harness-version` to pin the agent version installed in the image
- `--preserve-container` to keep the container after it exits (containers are removed on exit by default)
- `--dry-run` to print commands without executing them
