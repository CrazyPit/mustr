# mustr

Command center for coding-agent work — a CLI for organizing projects,
worktrees, and agent sessions. Data lives under `~/.mustr/`.

Early work in progress. Current surface: `mustr project add | rm | rename | list | default`
(alias `p`).

## Dirs

Each project holds a flat set of folders. Three reserved ones — `main`,
`pinned`, and `trash` (for archived items) — always exist; you manage the rest:

```sh
mustr dir list                  # alias: d, ls
mustr dir add notes
mustr dir rename notes archive
mustr dir rm archive --yes
mustr dir add deploy -p webapp  # target a project other than the default
```

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
