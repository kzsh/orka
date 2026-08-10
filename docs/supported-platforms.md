# Supported platforms

## Linux

Fully supported. Pre-built binaries are provided for x86\_64 and ARM64, in both glibc and musl variants. The Docker, Podman, and bubblewrap backends are available.

## macOS

Alpha. Apple silicon on macOS 26 or later, using the [Apple container](https://github.com/apple/container) backend:

```sh
orka --engine container
```

No macOS binary is distributed yet; build from source with `cargo build --release`. Intel Macs and earlier macOS releases are not supported, and neither is the bubblewrap backend.

The Docker backend is not restricted to Linux either. With Docker Desktop installed and `docker` on `PATH`, `orka --engine docker` should work on macOS; it is untested, and the macOS requirements above apply only to the Apple container backend.

The browser bundled into the image differs by architecture. On x86\_64 it is Chrome for Testing, downloaded by `agent-browser install`. Google publishes no Linux ARM64 build of Chrome, so ARM64 images use the Chromium build distributed by Playwright instead. Both are exposed at `/opt/browser-cache/chrome` through `AGENT_BROWSER_EXECUTABLE_PATH`, so browser tool calls work the same way. Chromium omits the proprietary codecs (H.264, AAC) that Chrome ships.

## Windows

Not supported. No Windows binary is distributed. Windows users can try [WSL2](https://learn.microsoft.com/en-us/windows/wsl/install), but this is untested.
