# What is orka

Coding agents run as your user, with access to your full home directory, your credentials, your shell history, and anything else reachable from your account. Most of the time that is more access than the task requires.

orka narrows that surface. It builds a container image for the agent runtime of your choice and runs each session inside it. You decide exactly what gets mounted in. Nothing else is reachable.

A secondary benefit is reproducibility: the agent environment is defined by the image and your explicit mounts, not by whatever happens to be in your PATH or shell config at the time.

---

## Use cases

### Work on a project

```sh
cd ~/src/my-project
orka
```

Mounts the current directory into the container at the same path and starts the agent there. The agent can read and write files within the project; nothing outside it is visible.

---

### Work on a single file

```sh
orka --file notes/spec.md
```

Only that file is mounted. Useful when you want the agent to work on one document without access to the surrounding repository.

Multiple files can be specified:

```sh
orka --file src/main.rs --file Cargo.toml
```

---

### Continue the last session

Arguments after `--` (or any unrecognized trailing flags) are passed verbatim to the agent. For `pi`, `-c` resumes the previous conversation:

```sh
orka -c
```

---

### Choose a runtime

```sh
orka --runtime claude
orka --runtime codex
orka --runtime pi       # default
```

The image for each runtime is built and cached independently.

---

### Start from a clean temporary directory

```sh
orka --tmp
```

Creates a temporary directory with `mktemp -d`, mounts it as the workdir, and starts the agent there. Nothing from the host project tree is mounted. The directory persists after the container exits so you can retrieve any output.

---

### Use a named scratch space

```sh
orka --scratchpad research
```

Creates (or reuses) `~/.local/share/orka/scratch/research` and mounts it as the workdir. Useful for ongoing tasks that are not tied to a source repository. The name is arbitrary; run the same name again to resume where you left off.
