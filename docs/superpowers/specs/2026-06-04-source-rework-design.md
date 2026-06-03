# Source rework — design

## Motivation

Today a project source is typed at registration (`add-git` → worktree-only,
`add-dir` → symlink-only), and `mount::add` derives the materialization kind from
that type. This couples *what is registered* to *how it is brought into a
workspace*. The same git repo cannot be a symlink in one workspace and a worktree
in another, and there is no way to attach an ad-hoc path that isn't a registered
source.

Decision: a source is **just a path**. The materialization kind (symlink vs
worktree) is chosen per workspace, explicitly, by the user.

## Project level — `source`

A source loses `kind` and `base_branch`. It is only `{ slug, path }`.

```
mustr source add <path> [slug]      # register a path (dir or git repo, undifferentiated)
mustr source ls | rm | rename       # unchanged
```

- `add` validates the path exists and is a directory. No git probing at
  registration; whether it's a git repo is decided lazily at worktree time.
- `add-git` / `add-dir` are removed.
- `ls` may annotate `(git)` by probing the path, but does not store it.

`sources.toml` entry shape becomes `{ path }` (keyed by slug). Old entries with
`kind` / `base_branch` are read tolerantly (extra fields ignored) so existing
data keeps loading.

## Workspace level — `w source`

The positional argument is a **target**: a registered source slug *or* a raw
filesystem path (ad-hoc). Resolution rule:

1. If the argument matches a registered source slug → use that source's path.
2. Otherwise → treat it as a filesystem path; canonicalize it. Error if it does
   not exist.

Ad-hoc paths are **not** written to the project registry; the mount is visible in
`w source ls` regardless because mounts are read from the filesystem. The mount
slug for an ad-hoc path is the slugified directory name.

```
mustr w source add <target>                 # symlink (alias: add-dir)
mustr w source add-dir <target>             # symlink (explicit)
mustr w source add-worktree <target> [--branch b] [--base-branch x]
                                            # worktree; error if target is not a git repo
mustr w source create-worktree <slug> [--branch b]
                                            # convert an existing symlink mount -> worktree, in place
mustr w source add --all                    # symlink every registered source
mustr w source ls | rm                      # unchanged
```

Behaviors:

- **add / add-dir** — symlink the resolved path into `src/<slug>`. Works for any
  directory (including a git repo, if you want a shared working copy).
- **add-worktree** — require the resolved path to be a git work tree (else a clear
  error). Branch defaults to the workspace slug; base branch is detected on the
  fly (`detect_base_branch`) unless `--base-branch` is given.
- **create-worktree** — operates on an existing mount in `src/` that is a symlink.
  Read its target, require it to be a git repo, remove the symlink, and create a
  worktree at the same path on the workspace-slug branch (or `--branch`). Reverse
  conversion (worktree → symlink) is intentionally not provided.
- **--all** — only on `add` (symlink), since worktrees are a per-source choice of
  branch/git. Symlinking every registered source is always valid.

Mount state stays filesystem-derived (no manifest); `list`, `remove`,
`repair_worktrees`, `remove_worktrees` are unaffected by the model change.

## base_branch

Dropped from `Source`. Detected at `add-worktree` time via the existing
`detect_base_branch`, overridable with `--base-branch`.

## Migration

Only the dogfooding `mustr` project has data. Tolerant deserialization (ignore
unknown `kind` / `base_branch` fields) is enough; no migration script. If any
registered source relied on `base_branch`, it is simply re-detected at worktree
time.

## Test plan (TDD)

Library:
- `source::add` registers a path with `{slug, path}`; default slug from basename;
  errors on missing path / non-dir / duplicate slug.
- `source` load tolerates legacy entries with `kind`/`base_branch`.
- `mount::add` link mode: registered slug → symlink; ad-hoc path → symlink with
  basename slug.
- `mount::add` worktree mode: registered git slug → worktree on ws branch;
  ad-hoc git path → worktree; non-git target → error.
- target resolution: registry hit wins; unknown slug with no such path → error.
- `mount::convert_to_worktree`: symlink mount → worktree on ws branch; non-symlink
  or non-git target → error.

CLI:
- `source add <path>` (no more add-git/add-dir).
- `w source add <path>` ad-hoc symlink; `w source add-worktree <path>` ad-hoc
  worktree; `w source create-worktree <slug>` conversion.

## Out of scope

- Auto-registering ad-hoc paths as project sources (kept one-off by design).
- Worktree → symlink conversion.
- Raw-path `--all`.
