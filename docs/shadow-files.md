# Shadow files

An `orkashadow` file lists paths that should be hidden from the agent. Any file
matched by its patterns is replaced with an empty read-only stub inside the
container. The agent can see that the file exists but cannot read or write its
content.

There are two kinds of shadow file:

- **Global** (`~/.config/orka/orkashadow`): applies to every directory mounted in any orka session on this machine.
- **Per-repo** (`.orkashadow` at the root of a mounted directory): applies only to that directory.

Both use `.gitignore` syntax (thanks [BurntSushi](https://github.com/BurntSushi/ripgrep/tree/master/crates/ignore)). Per-repo patterns are evaluated after global ones and can negate a global match with `!`.

## Syntax

The syntax is identical to `.gitignore`. A few common forms:

| Pattern | What it matches |
|---|---|
| `.env` | A file named exactly `.env` at any level |
| `**/.env` | `.env` files at any depth |
| `*.key` | Any file ending in `.key` |
| `secrets/` | Everything inside a directory named `secrets` |
| `src/pricing/` | Everything inside `src/pricing/` |
| `!important.key` | Un-shadows a file matched by an earlier pattern |

## Setting up the global file

Copy the bundled template:

```sh
mkdir -p ~/.config/orka
curl -Lo ~/.config/orka/orkashadow \
  https://raw.githubusercontent.com/kzsh/orka/main/config/orkashadow
```

The template ships with all patterns commented out. Uncomment the ones that
apply to your setup. Patterns in this file will apply to every project you
work on, so keep it to things that are sensitive on any codebase — credentials,
key files, and the like.

## Common global patterns

Uncomment any of these in `~/.config/orka/orkashadow`:

```
# environment variable files
.env
**/.env

# key and certificate files
*.pem
*.key
*.p12
*.pfx

# secrets directories
secrets/
.secrets/
```

## Adding a per-repo file

For files or directories that are sensitive only in a specific project, place a
`.orkashadow` file at the root of the repository:

```sh
touch .orkashadow
```

Add patterns the same way you would in a `.gitignore`. For example, to hide a
directory containing proprietary business logic:

```
src/pricing/
```

Or to hide a specific file:

```
config/production.yaml
```

## Negating a global pattern

If the global file shadows `*.key` but a particular project has a `.key` file
the agent legitimately needs to read, add a negation in the per-repo
`.orkashadow`:

```
!public.key
```

Per-repo patterns are applied after global ones, so the negation takes effect.

## Verifying

Run with `--dry-run` to see what the container command looks like without
executing it:

```sh
orka --dry-run
```

Shadow volumes appear as additional `-v` flags with a read-only empty file
mounted over the matched path. If a path you expected to be shadowed is not
listed, check that the pattern matches relative to the root of the mounted
directory.

See [how it works](how-it-works.md) for the mechanism behind shadow mounts, or
[getting started](getting-started.md) for initial setup.
