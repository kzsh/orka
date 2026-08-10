# Custom base image

Orka builds a base image (`orka-base`) from an embedded `Dockerfile.base` before building the agent image on top of it. To replace that base, place your own `Dockerfile.base` at:

```
~/.config/orka/Dockerfile.base
```

When this file is present, orka uses it instead of the embedded default. To revert to the default temporarily, rename or remove the file.

The base image is built with the Docker layer cache and is shared across all harnesses, so it is only rebuilt when it changes. The agent layer on top — which installs pi, claude, or codex — rebuilds independently.

## Why you would do this

- Your organization requires a specific base OS (RHEL, Ubuntu, Alpine).
- You want to add tools or configuration to every agent session without writing a preset for them.
- You want to strip agent-browser and Chromium from the image to reduce size.

## What the default base provides

The embedded `Dockerfile.base` installs:

- A broad set of Linux development tools (`git`, `curl`, `jq`, `ripgrep`, `fd-find`, `fzf`, `gh`, `python3`, and others)
- Bun (used by the pi and claude harnesses to install the agent)
- agent-browser and its Chromium dependency (used by the pi harness for browser tool calls)

## Minimum requirements per harness

Your custom base must provide whatever the agent harness layer needs at build time. The harness layer does not install a package manager — it assumes the base has what is needed.

| Harness | Required in base |
|---|---|
| `pi` | `bun` at `/usr/local/bin/bun` |
| `claude` | `bun` at `/usr/local/bin/bun` |
| `codex` | `curl`, `jq`, `tar` |

All harnesses also require `groupadd`, `useradd`, and `bash`. These are present on any standard Linux distribution.

## The `AGENT_BROWSER_EXECUTABLE_PATH` convention

`agent-browser install` downloads Chrome for Testing into `$HOME/.agent-browser/browsers` of whichever user runs it — root during the image build, which the runtime user cannot read. The default base therefore installs under a throwaway `HOME`, moves the download to `/opt/browser-cache`, and points every user at it:

```dockerfile
RUN HOME=/tmp/agent-browser-install agent-browser install && \
    mv /tmp/agent-browser-install/.agent-browser/browsers/* /opt/browser-cache/ && \
    ln -s "$(find /opt/browser-cache -maxdepth 2 -type f -name chrome | head -n 1)" /opt/browser-cache/chrome && \
    chmod -R a+rX /opt/browser-cache

ENV AGENT_BROWSER_EXECUTABLE_PATH="/opt/browser-cache/chrome"
```

If your custom base includes agent-browser, do the same. Any readable path works as long as `AGENT_BROWSER_EXECUTABLE_PATH` names the Chrome binary. If your base omits agent-browser, omit the variable.

`agent-browser install` only works on x86\_64: Google publishes no Linux ARM64 build of Chrome for Testing, and the command exits nonzero there. The default base fetches Playwright's ARM64 Chromium build instead, and a custom base intended for ARM64 needs an equivalent step:

```dockerfile
RUN curl -fsSL "https://cdn.playwright.dev/dbazure/download/playwright/builds/chromium/1237/chromium-linux-arm64.zip" -o /tmp/chromium.zip && \
    unzip -q /tmp/chromium.zip -d /opt/browser-cache && \
    rm -f /tmp/chromium.zip && \
    ln -s /opt/browser-cache/chrome-linux/chrome /opt/browser-cache/chrome
```

Revision numbers are listed in Playwright's `packages/playwright-core/browsers.json`. A distribution Chromium package works equally well; point the variable at it.

## Example: RHEL / dnf

```dockerfile
FROM registry.access.redhat.com/ubi9/ubi:latest

RUN dnf install -y \
      bzip2 \
      curl \
      git \
      jq \
      python3 \
      ripgrep \
      tar \
      unzip \
    && dnf clean all

ARG BUN_VERSION=1.3.14

RUN set -eux && \
    ARCH=$(uname -m) && \
    case "$ARCH" in \
      x86_64)  TARGET="bun-linux-x64" ;; \
      aarch64) TARGET="bun-linux-aarch64" ;; \
      *) echo "Unsupported architecture: $ARCH" >&2; exit 1 ;; \
    esac && \
    curl -fsSL "https://github.com/oven-sh/bun/releases/download/bun-v${BUN_VERSION}/${TARGET}.zip" \
      -o /tmp/bun.zip && \
    unzip /tmp/bun.zip -d /tmp && \
    mv "/tmp/${TARGET}/bun" /usr/local/bin/bun && \
    chmod +x /usr/local/bin/bun && \
    rm -rf /tmp/bun.zip "/tmp/${TARGET}" && \
    ln -sf /usr/local/bin/bun /usr/local/bin/node && \
    ln -sf /usr/local/bin/bun /usr/local/bin/npm && \
    ln -sf /usr/local/bin/bun /usr/local/bin/npx
```

This gives you a RHEL-based image suitable for all three harnesses. It omits agent-browser and Chromium — pi will still work but browser tool calls will fail at runtime.

## Example: Debian without agent-browser

Start from the same Debian base but skip the agent-browser and Chromium layers:

```dockerfile
FROM debian:bookworm-slim

ARG BUN_VERSION=1.3.14

ENV DEBIAN_FRONTEND=noninteractive

RUN apt-get update && apt-get install -y \
      curl \
      git \
      jq \
      python3 \
      ripgrep \
      unzip \
    && rm -rf /var/lib/apt/lists/*

RUN set -eux && \
    ARCH=$(uname -m) && \
    case "$ARCH" in \
      x86_64)  TARGET="bun-linux-x64" ;; \
      aarch64) TARGET="bun-linux-aarch64" ;; \
      *) echo "Unsupported architecture: $ARCH" >&2; exit 1 ;; \
    esac && \
    curl -fsSL "https://github.com/oven-sh/bun/releases/download/bun-v${BUN_VERSION}/${TARGET}.zip" \
      -o /tmp/bun.zip && \
    unzip /tmp/bun.zip -d /tmp && \
    mv "/tmp/${TARGET}/bun" /usr/local/bin/bun && \
    chmod +x /usr/local/bin/bun && \
    rm -rf /tmp/bun.zip "/tmp/${TARGET}" && \
    ln -sf /usr/local/bin/bun /usr/local/bin/node && \
    ln -sf /usr/local/bin/bun /usr/local/bin/npm && \
    ln -sf /usr/local/bin/bun /usr/local/bin/npx
```

## Verifying

Use `--dry-run` to confirm orka is picking up your file:

```sh
orka --dry-run
```

The base image build command will show `--file` pointing at a temporary copy of your `Dockerfile.base`. If you see the embedded default instead, check that the file is at `~/.config/orka/Dockerfile.base`.
