# mustr

Command center for coding-agent work — a CLI for organizing projects,
worktrees, and agent sessions. Data lives under `~/.mustr/`.

Early work in progress. Current surface: `mustr project add | rm | rename | list`
(alias `p`).

## Status

`mustr` with no command (or `mustr status`) prints a context overview: outside a
project it lists every project with rollup counts and totals; at a project root
it shows the dirs and their workspaces (trash by count only); inside a workspace
it shows the materialized sources, agents with live status, and `docs/`
artifacts.

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
mustr a ls                  # agents in the cwd workspace (dir/ws/slug + status)
mustr a ls -p               # widen to every agent in the project
mustr a ls -a               # only running agents (--active)
mustr a close review        # terminate a running agent (SIGTERM; -f = SIGKILL)
mustr a close main/tb-1/review  # address any agent by its `a ls` path
mustr a mv main review      # rename
mustr a rm review -f        # remove the record (session transcript untouched)
```

`a ls` lists the cwd (or `-w`) workspace's agents; `-p` (bare, or `-p <slug>`)
widens it to the whole project, and outside any workspace it spans the project
anyway. `open`/`close`/`rm`/`rename` take an agent address `[[dir/]ws/]slug`: a
bare slug acts on the cwd (or `-w`) workspace, while a `ws/slug` or `dir/ws/slug`
address — exactly what `a ls` prints — targets any workspace in the project, so
list rows paste straight back.

Supported kinds: `claude`, `codex`, `cursor`. A new agent's kind defaults to the
project's `default_agent` (`~/.mustr/projects/<p>/config.toml`), then the global
`default_agent`, else `claude`; an existing agent keeps its own kind. Workspace
comes from the cwd, or `-w [dir/]slug` / `-p`.

## Config

Global settings live in `~/.mustr/config.toml`:

```sh
mustr config                          # list all keys and values
mustr config default_agent codex      # set
mustr config default_agent            # get
mustr config default_agent --unset    # revert to default
```

| Key | Values | Effect |
|-----|--------|--------|
| `default_agent` | `claude`\|`codex`\|`cursor` | Fallback kind for `agent open` when `--type` and the project both omit it |

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
