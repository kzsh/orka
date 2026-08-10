# Contributing

## Building from source

The pinned toolchain is in `rust-toolchain.toml`; rustup installs it automatically.

```sh
cargo build --release
# binary at target/release/orka
```

The Dockerfiles (`Dockerfile`, `Dockerfile.base`, `Dockerfile.claude`,
`Dockerfile.codex`), the entrypoint scripts, and the config templates under
`config/` are embedded in the binary at compile time via `include_str!`. The
released binary is self-contained, and changing any of those files requires a
rebuild.

## Tests

```sh
cargo test
```

## Source layout

| Path | Contents |
|---|---|
| `src/cli.rs` | Clap definitions: every flag and subcommand |
| `src/docker.rs` | `RunConfig`, image tag logic, engine command builders |
| `src/bwrap.rs` | Bubblewrap backend |
| `src/config.rs` | `config.yaml` and `environments.yaml` parsing |
| `src/expand.rs` | `~` and `$VAR` expansion for config values |
| `src/shadow.rs` | `orkashadow` matching and stub mounts |
| `src/scratchpad.rs` | Scratchpad directory handling and selector |
| `config/` | Templates written by `orka config init` |
| `docs/` | User documentation published with the repo |

## Release scripts

| Script | Purpose |
|---|---|
| `scripts/build.sh` | Cross-compile all Linux targets into `dist/` (requires Docker and `cross`) |
| `scripts/build-macos.sh` | Build the Apple silicon binary on a remote Mac (`ORKA_BUILD_HOST`) |
| `scripts/mac-cargo.sh` | Run arbitrary cargo commands on that remote Mac |
| `scripts/gen-licenses.sh` | Regenerate `THIRD_PARTY_LICENSES` (requires `cargo-about`) |
| `scripts/publish.sh` | Upload `dist/` artifacts to GitHub releases (requires `gh`) |

`THIRD_PARTY_LICENSES` is committed and embedded in the binary. Regenerate it
whenever dependencies change.

## Documentation

User-facing changes belong in `CHANGELOG.md` under `latest`, and in `README.md`
or the relevant page under `docs/` when flags, subcommands, defaults, config
keys, or supported platforms change.

## License

Contributions are accepted under the MIT license; see `LICENSE`.
