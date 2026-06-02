# mustr

Command center for coding-agent work — a CLI for organizing projects,
worktrees, and agent sessions. Data lives under `~/.mustr/`.

Early work in progress. Current surface: `mustr project add | rm | rename | list | default`
(alias `p`).

## Select the active project

`mustr project default <slug>` (aliases `take`, `select`) marks a project as the
default. With no slug it opens an interactive picker. It prints the project's
path to stdout, so a shell wrapper can jump into it:

```sh
mcd() { cd "$(mustr p default "$@")"; }
```

## Develop

```bash
cargo test        # run the suite
cargo run -- p ls # run from source
```

## Install

Runs the tests, builds a release binary, and copies it to `~/bin/`:

```bash
cargo xtask install
```

Make sure `~/bin` is on your `PATH`.
