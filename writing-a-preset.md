# Writing a preset

A preset is a named block in `~/.config/orka/environments.yaml` that mounts host directories into the container and optionally injects environment variables. This guide walks through writing one from scratch, using bun as the example.

## Step 1: find what needs to be mounted

The goal is to identify every directory on the host that the tool needs in order to run inside the container. Start by asking where the tool lives and what it depends on.

For bun, the binary and its runtime data all live under one directory:

```sh
which bun
# /home/user/.bun/bin/bun

echo $BUN_INSTALL
# /home/user/.bun

ls ~/.bun
# bin/  install/
```

`~/.bun` contains the binary, installed global packages, and internal cache. Mounting that one directory is enough for the agent to invoke bun and use any globally installed tools.

Some tools scatter state across multiple locations. Check the tool's documentation or inspect with:

```sh
# See open files while the tool is running
lsof -p $(pgrep bun)

# Or trace filesystem access (Linux)
strace -e trace=openat -p $(pgrep bun) 2>&1 | grep -v ENOENT
```

For bun, one mount is sufficient.

## Step 2: decide on read-only vs read-write

For each mount, decide whether the agent should be able to write back to the host path.

- **Read-only (`:ro`)**: the tool's installation directory, the binary itself, shared caches that you do not want the agent modifying. Safer default.
- **Read-write** (no suffix): necessary when the agent needs to install packages globally, write to a cache, or persist state that should survive the session.

For bun, the question is whether you want the agent to be able to install global packages that persist to your host `~/.bun`. In most cases you do not — the agent should install project dependencies into the project directory, not globally. Make it read-only:

```yaml
volumes:
  - ~/.bun/:~/.bun/:ro
```

If you have a specific workflow where the agent needs to run `bun install --global`, remove the `:ro`.

## Step 3: check whether PATH needs updating

The container starts with a minimal PATH. If the tool's binary is not in a standard location like `/usr/bin` or `/usr/local/bin`, you need to add its directory to PATH explicitly.

Check where bun's binary sits:

```sh
which bun
# /home/user/.bun/bin/bun
```

`~/.bun/bin` is not on the default container PATH, so without an explicit env entry bun will not be found inside the container. Add it:

```yaml
env:
  - PATH=~/.bun/bin:$PATH
```

orka expands `$PATH` from your host environment at runtime, so this prepends `~/.bun/bin` to whatever PATH the container already has.

## Step 4: write the preset

Open `~/.config/orka/environments.yaml` and add a new block under `environments`:

```yaml
environments:
  bun:
    volumes:
      - ~/.bun/:~/.bun/:ro
    env:
      - PATH=~/.bun/bin:$PATH
```

The `~` prefix is expanded to `$HOME` at runtime — you do not need to write the full path.

## Step 5: verify

Run with the preset and confirm the tool is available inside the container:

```sh
orka --preset bun -- bun --version
```

If the command is not found, the binary's directory is not on PATH. If it errors on a missing file, a required directory is not mounted. Add what is missing and retry.

## Stacking presets

Presets compose. If your project uses bun and also needs a language runtime from another preset, pass both:

```sh
orka --preset bun --preset python
```

Volumes and env vars from all named presets are merged in order.

See [getting started](getting-started.md) for initial setup, or [how it works](how-it-works.md) for background on how mounts and the container environment are constructed.
