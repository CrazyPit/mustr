<div align="center">

# `mustr`

**A command center for coding-agent work.**

One CLI to organize projects, git worktrees, and AI coding-agent sessions —
so you can run many agents across many tasks without losing the thread.

![status](https://img.shields.io/badge/status-WIP-f5a623?style=flat-square)
![rust](https://img.shields.io/badge/Rust-2024-dea584?style=flat-square&logo=rust&logoColor=white)
![interface](https://img.shields.io/badge/CLI-first-4c8bf5?style=flat-square)

</div>

```text
  mustr · webapp / main / login-bug
  ────────────────────────────────────────────
  Fix login redirect after timeout

  sources    backend  worktree  login-bug
             shared   link      ~/notes
  agents     ● main   claude    running · pid 4217
             ○ review codex     idle
  artifacts  design.md
             plan.md

  ~/.mustr/projects/webapp/main/login-bug
```

> [!NOTE]
> Early but usable. `mustr` is **CLI-first**; a GUI will layer on the same
> library later, which is why every command's data is assembled in the library
> and only *rendered* by the CLI.

---

## ✨ Why

Running coding agents at scale gets messy fast: which branch is this agent on,
which task was that session about, is that one still running? `mustr` gives every
task its own **workspace** — an isolated folder with its own git worktrees, its
own docs, and its own named agent sessions that resume exactly where you left off.

| | |
|---|---|
| 🗂️ **Workspaces per task** | Each task gets isolated git worktrees + a scratch area, scaffolded automatically. |
| 🤖 **Agents that resume** | `claude`, `codex`, `cursor` — sessions are pinned and resumed; a pid lock blocks double-launches. |
| 🌳 **Worktrees, handled** | Materialize a repo as a worktree (branch per workspace) or a dir as a symlink — and they're repaired/cleaned on move & delete. |
| 🧭 **Context from your cwd** | No hidden "current" state — where you `cd` *is* the context. Override with `-p` / `-w`. |
| 👀 **One glance** | Bare `mustr` prints a context-aware overview: projects, workspaces, live agents. |

---

## 🚀 Quick start

```bash
cargo xtask install          # build + test, then copy the binary to ~/bin/mustr
mustr project add webapp     # create your first project
mustr                        # see where you are
```

> [!TIP]
> Make sure `~/bin` is on your `PATH`. Most subcommands have short aliases:
> `p` project · `d` dir · `w` workspace · `src` source · `a` agent.

---

## 📖 A day with `mustr`

A walk through the whole flow — create a project, wire up code, spin up tasks,
and turn agents loose.

### 1 · Start a project

```console
$ mustr project add webapp
Created project webapp
```

Everything hangs off a project. They live under `~/.mustr/projects/`.

### 2 · Register your code as **sources**

A source is just a directory (a git repo or a plain folder) you'll pull into
workspaces later. How it's brought in — symlink or worktree — is your choice at
materialization time, not fixed at registration.

```console
$ mustr src add ~/code/backend            # a git repo
Added source backend -> /Users/you/code/backend

$ mustr src add ~/notes shared
Added source shared -> /Users/you/notes
```

> [!NOTE]
> Sources are just a registry — your real repos and dirs are never touched.

### 3 · Spin up a **workspace** per task

```console
$ mustr w add login-bug -d "Fix login redirect after timeout"
Created workspace main/login-bug in webapp

$ cd "$(mustr path login-bug)"
```

Each workspace is scaffolded with `src/` (your code), `docs/` (artifacts), and
`agents/` (sessions) — plus an `AGENTS.md` so any agent you launch knows the layout.

### 4 · Materialize sources into it

You choose how each source lands in `src/`: a **symlink** (share the directory)
or a **worktree** (an isolated git branch named after the workspace).

```console
$ mustr w src add-worktree backend    # git worktree on branch login-bug
Added worktree backend on login-bug

$ mustr w src add shared              # symlink the shared notes dir
Linked shared

$ mustr w src add --all               # symlink every registered source
```

Found a bug in a dependency you didn't register? Attach it ad-hoc by path and
patch it on its own branch:

```console
$ mustr w src add-worktree ~/code/some-lib
Added worktree some-lib on login-bug
```

### 5 · Launch **agents**

```console
$ mustr a open                    # default agent `main` (kind: claude)
starting claude 'main'

$ mustr a open review -t codex    # a second agent, this one codex
starting codex 'review'
```

`mustr` pins each agent's session id and **resumes** it next time you open it —
and refuses to open one that's already running.

### 6 · See everything at a glance

```console
$ mustr
```
```text
  mustr · webapp / main / login-bug
  ────────────────────────────────────────────
  Fix login redirect after timeout

  sources    backend  worktree  login-bug
             shared   link      ~/notes
  agents     ● main   claude    running · pid 4217
             ○ review codex     idle
  artifacts  design.md

  ~/.mustr/projects/webapp/main/login-bug
```

### 7 · Manage agents across the project

```console
$ mustr a ls -p                   # every agent in the project
$ mustr a close review            # SIGTERM a running one (-f = SIGKILL)
Closed agent review (pid 4220)
```

> [!TIP]
> `a ls` shows the *cwd workspace*; `-p` widens it to the whole project. You can
> address any agent by the path the list prints: `mustr a close main/login-bug/review`.

### 8 · Wrap up

```console
$ mustr w rm login-bug            # -> trash (reversible)
Moved main/login-bug to trash

$ mustr w purge -y                # empty the trash for good
Purged 1 workspace
```

---

## 🧱 The model

```text
project            webapp
└─ dir             main · pinned · trash · …your own
   └─ workspace    login-bug                one per task
      ├─ src/      worktrees + symlinks  ◄── materialized from sources
      ├─ docs/     artifacts
      └─ agents/   claude · codex · cursor
```

- **project** — top-level container (`~/.mustr/projects/<project>/`).
- **dir** — a folder/category inside a project. `main`, `pinned`, and `trash`
  always exist; add your own.
- **workspace** — one task. Addressed as `[dir/]slug` (dir defaults to `main`).
- **source** — an external repo/dir registered on the project.
- **mount** — a source materialized into a workspace's `src/` (worktree or symlink).
- **agent** — a named coding-agent session living in a workspace.

---

## 🧭 Context

There is no stored "current" project or workspace — **context comes from your
working directory**. Run a command from inside `~/.mustr/projects/<project>/…`
and it targets that project (and workspace, if you're in one).

| Override | Meaning |
|---|---|
| `-p, --project <slug>` | Act on another project (`a`: bare `-p` = the cwd project) |
| `-w, --workspace [dir/]slug` | Act on another workspace |
| `mustr path [dir/]slug` | Print a workspace path, e.g. `cd "$(mustr path login-bug)"` |

Listings mark the current project/workspace with `★`.

---

## 📚 Command reference

<details>
<summary><b>project</b> — top-level containers (alias <code>p</code>)</summary>

```bash
mustr project add <name>
mustr project list                 # alias: ls
mustr project rename <slug> <new>
mustr project rm <slug> --yes
```
</details>

<details>
<summary><b>dir</b> — folders inside a project (alias <code>d</code>)</summary>

```bash
mustr dir list
mustr dir add notes
mustr dir rename notes archive
mustr dir rm archive --yes
mustr dir add deploy -p webapp     # target a project other than the cwd one
```
`main`, `pinned`, and `trash` are reserved and always present.
</details>

<details>
<summary><b>source</b> — register external repos & dirs (alias <code>src</code>)</summary>

```bash
mustr source ls                     # `(git)` marks repos that can be worktrees
mustr src add /path/to/dir [slug]   # a git repo or a plain dir, undifferentiated
mustr src rename backend api        # alias: mv
mustr src rm backend                # entry only; the real repo/dir is untouched
```
</details>

<details>
<summary><b>workspace</b> — one folder per task (alias <code>w</code>)</summary>

```bash
mustr w add dark-mode -d "Add dark theme toggle"
mustr w ls                          # pinned, then newest dirs, then main; trash hidden
mustr w ls --all                    # include trash
mustr w ls main                     # one dir, unprefixed
mustr w grep login                  # search slug + description
mustr w mv dark-mode pinned         # move to another dir
mustr w rename dark-mode dark-theme # rename slug
mustr w rename dark-mode -d "..."   # set description
mustr w rm dark-mode                # -> trash (reversible)
mustr w rm trash/dark-mode -y       # permanent
mustr w purge -y                    # empty trash
```

**Materializing sources** into a workspace's `src/`:

The target of a `w src` command is a registered source slug **or** a raw path
(ad-hoc, not registered):

```bash
mustr w src add backend                  # symlink (alias: add-dir)
mustr w src add-worktree backend         # git worktree on branch <workspace>
mustr w src add-worktree backend --branch x --base-branch main
mustr w src add-worktree ~/code/some-lib # ad-hoc: attach a path not registered
mustr w src create-worktree backend      # convert an existing symlink -> worktree
mustr w src add --all                    # symlink every registered source
mustr w src ls
mustr w src rm backend -f                # -f force-removes a dirty worktree
```
</details>

<details>
<summary><b>agent</b> — coding-agent sessions (alias <code>a</code>)</summary>

```bash
mustr a o                   # open the default agent `main` (o = open)
mustr a o review            # a second agent in the same workspace
mustr a o x -t codex        # set kind for a new agent (-t/--type)
mustr a ls                  # agents in the cwd workspace (dir/ws/slug + status)
mustr a ls -p               # widen to every agent in the project
mustr a ls -a               # only running agents (--active)
mustr a close review        # terminate a running agent (SIGTERM; -f = SIGKILL)
mustr a close main/login-bug/review   # address any agent by its `a ls` path
mustr a mv main review      # rename
mustr a rm review -f        # remove the record (session transcript untouched)
```

Kinds: `claude`, `codex`, `cursor`. A new agent's kind defaults to the project's
`default_agent`, then the global one, else `claude`; an existing agent keeps its
own kind. An agent address is `[[dir/]ws/]slug` — a bare slug uses the cwd (or
`-w`) workspace, a prefixed one targets any workspace in the project.
</details>

<details>
<summary><b>config</b> — global settings (<code>~/.mustr/config.toml</code>)</summary>

```bash
mustr config                          # list all keys and values
mustr config default_agent codex      # set
mustr config default_agent            # get
mustr config default_agent --unset    # revert to default
```

| Key | Values | Effect |
|-----|--------|--------|
| `default_agent` | `claude`·`codex`·`cursor` | Fallback kind for `agent open` when `--type` and the project both omit it |
</details>

---

## 🌳 How worktrees are handled

When you materialize a git source, `mustr` creates a worktree checked out on a
branch named after the workspace. The tricky part is keeping git's two-way
worktree link intact as workspaces move around — `mustr` does it for you:

- **move / rename** a workspace → worktree links are **repaired** in place.
- **permanent delete** → worktrees are **removed** via git (the branch is kept).
- soft-delete to trash is reversible; the worktree comes back with the workspace.

---

## 🛠️ Develop

```bash
cargo test          # run the suite
cargo run -- p ls   # run from source
cargo xtask install # test, build --release, copy to ~/bin/
```

Architecture: a `mustr` **library** holds all the logic and assembles data
(projects, workspaces, agents, the status overview); the **binary** is a thin
clap front-end that renders it. That split is what lets a future GUI reuse
everything underneath.
