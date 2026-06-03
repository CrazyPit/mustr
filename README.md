# mustr

Command center for coding-agent work — a CLI for organizing projects,
worktrees, and agent sessions. Data lives under `~/.mustr/`.

Early work in progress. Current surface: `mustr project add | rm | rename | list`
(alias `p`).

## Context

There is no stored "current" project or workspace — context comes from your
working directory. Run a command from inside `~/.mustr/projects/<project>/…` and
it targets that project (and workspace, if you're in one). Override with
`--project/-p <slug>` and `--workspace/-w [dir/]slug`. `mustr path <ws>` prints a
workspace path so you can `cd "$(mustr path tb-123)"` into it. Listings mark the
current project/workspace with `★`.

## Dirs

Each project holds a flat set of folders. Three reserved ones — `main`,
`pinned`, and `trash` (for archived items) — always exist; you manage the rest:

```sh
mustr dir list                  # alias: d, ls
mustr dir add notes
mustr dir rename notes archive
mustr dir rm archive --yes
mustr dir add deploy -p webapp  # target a project other than the cwd one
```

## Sources

A project keeps a registry of **sources** — external git repos and directories —
in `sources.toml`. Later, workspaces materialize these into their `src/` as
worktrees and symlinks.

```sh
mustr source ls
mustr src add-git /path/to/repo [slug] [--base-branch main]   # base branch auto-detected
mustr src add-dir /path/to/dir [slug]
mustr src rm backend
mustr src rename backend api        # alias: mv
```

`rm` only drops the registry entry; the real repo/dir is untouched.

## Workspaces

Workspaces live inside a project's dirs, addressed as `[dir/]slug` (dir defaults
to `main`). `rm` soft-deletes into `trash`; permanent deletes are explicit. Each
new workspace is scaffolded with `src/` (worktrees + symlinks), `docs/`
(artifacts), and `agents/` (agent sessions and state), plus an `AGENTS.md`
orientation file (with a `CLAUDE.md` symlink) so any agent launched there knows
the layout.

```sh
mustr w add tb-123 -d "Fix bug in incognito mode"
mustr w ls                       # all dirs (pinned, then newest dirs, then main); trash hidden
mustr w ls --all                 # include trash (also --trash)
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

`mustr path [dir/]slug` prints a workspace's directory, so you can jump into it:

```sh
wcd() { cd "$(mustr path "$@")"; }
```

### Materializing sources

Inside a workspace, `mustr w src` brings project sources into `src/`: git
sources become worktrees (branch = the workspace slug by default), dir sources
become symlinks. The workspace comes from the cwd, or `-w [dir/]slug`.

```sh
mustr w src add backend            # worktree on branch <workspace>
mustr w src add backend --branch x # custom branch
mustr w src add --all              # materialize every project source
mustr w src ls
mustr w src rm backend -f          # -f skips confirm, force-removes a dirty worktree
```

## Agents

Open a coding agent in a workspace. mustr pins a stable Claude Code session id
per (workspace, agent), resumes it if a transcript exists, and refuses to open a
session that is already running.

```sh
mustr a o                  # open the default agent `main` (a = agent, o = open)
mustr a o review            # a second agent in the same workspace
mustr a o x -t claude       # set kind for a new agent (-t/--type, alias -a/--agent)
mustr a ls                  # records + running status
mustr a mv main review      # rename
mustr a rm review -f        # remove the record (session transcript untouched)
```

Supported kinds: `claude`, `codex`, `cursor`. A new agent's kind defaults to the
project's `default_agent` (`~/.mustr/projects/<p>/config.toml`), else `claude`;
an existing agent keeps its own kind. Workspace comes from the cwd, or
`-w [dir/]slug` / `-p`.

mustr runs the agent as a child in the workspace root, holding a pid lock so a
second `open` of the same agent is refused while it runs, and pins/recovers the
agent's session id so re-opening resumes the same conversation.

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
