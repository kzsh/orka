# Shell completions

`orka config completions <SHELL>` writes a completion script to stdout. Supported shells: `bash`, `zsh`, `fish`, `elvish`, `powershell`.

The script is generated from the same flag definitions the binary parses, so it stays in step with the installed version. Regenerate it after upgrading orka.

## bash

```sh
mkdir -p ~/.local/share/bash-completion/completions
orka config completions bash > ~/.local/share/bash-completion/completions/orka
```

The file is loaded on the next shell start, provided `bash-completion` is installed and sourced from `~/.bashrc`.

## zsh

Write the script to any directory on `$fpath` as `_orka`:

```sh
orka config completions zsh > "${fpath[1]}/_orka"
```

If `compinit` runs from `~/.zshrc`, completions are available in the next shell. Remove `~/.zcompdump` if a stale cache shadows the new file.

## fish

```sh
mkdir -p ~/.config/fish/completions
orka config completions fish > ~/.config/fish/completions/orka.fish
```

## elvish and powershell

Both emit to stdout for you to source or dot-source from your profile:

```sh
orka config completions elvish
orka config completions powershell
```

## Other config subcommands

| Command | Description |
|---|---|
| `orka config init` | Write `config.yaml`, `environments.yaml`, and `orkashadow` to `~/.config/orka/`. Existing files are skipped. |
| `orka config path` | Print the paths orka reads configuration from. |

See [getting started](getting-started.md) for what those files contain.
