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

## Workspaces

Workspaces live inside a project's dirs, addressed as `[dir/]slug` (dir defaults
to `main`). `rm` soft-deletes into `trash`; permanent deletes are explicit.

```sh
mustr w add tb-123 -d "Fix bug in incognito mode"
mustr w ls                       # all dirs, prefixed (main/tb-123)
mustr w ls main                  # one dir, unprefixed
mustr w grep incognito           # search slug + description
mustr w mv tb-123 pinned         # move to another dir
mustr w rename tb-123 tb-3434    # rename slug
mustr w rename tb-123 -d "..."   # set description
mustr w rm tb-123                # -> trash (reversible)
mustr w rm trash/tb-123 -y       # permanent
mustr w rm tb-123 -f -y          # skip trash, permanent
mustr w purge -y                 # empty trash
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
