# orka

**Latest:** 2026-07-09

## Install

Download the binary for your platform from [Releases](../../releases), make it executable, and put it on your `PATH`:

```sh
curl -Lo orka https://github.com/kzsh/orka/releases/latest/download/orka-x86_64-unknown-linux-musl
chmod +x orka
mv orka ~/.local/bin/
```

| File | Platform |
|---|---|
| `orka-x86_64-unknown-linux-gnu` | Linux x86\_64 (glibc) |
| `orka-x86_64-unknown-linux-musl` | Linux x86\_64 (static) |
| `orka-aarch64-unknown-linux-gnu` | Linux ARM64 (glibc) |
| `orka-aarch64-unknown-linux-musl` | Linux ARM64 (static) |

Prefer the `musl` variant unless you have a specific reason not to — it has no runtime dependencies.
