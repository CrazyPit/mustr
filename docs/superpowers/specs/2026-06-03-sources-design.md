# Spec — sources (iteration 6)

## Goal

A project-level registry of **sources** — pointers to external git repos or
plain directories — that later iterations materialize into workspace `src/`
folders (worktrees / symlinks). This iteration is the registry only.

## Commands (`src` = `source`)

```
mustr source ls
mustr src add-git <path> [slug] [--base-branch <branch>]
mustr src add-dir <path> [slug]
mustr src rm <slug>
mustr src rename <slug> <new-slug>        # alias: mv
```

`--project/-p` selects the project (default project otherwise), as elsewhere.

## Storage

One file per project at `~/.mustr/projects/<project>/sources.toml`, a table
keyed by slug:

```toml
[sources.backend]
kind = "git"
path = "/Users/me/code/backend"
base_branch = "main"

[sources.designs]
kind = "dir"
path = "/Users/me/Documents/designs"
```

The slug is the table key (not stored in the entry). `base_branch` only for git.

## Behavior

- **add-git**: path is canonicalized to absolute and must be a git work tree
  (`git rev-parse --is-inside-work-tree`), else error. Slug defaults to the
  slugified repo folder name. `base_branch` defaults to, in order:
  `origin/HEAD` → first existing of `main`/`master`/`develop`/`trunk` → current
  branch → `main`. `--base-branch` overrides.
- **add-dir**: path canonicalized to absolute and must be an existing directory,
  else error. Slug defaults to the slugified folder name. No `base_branch`.
- **rm**: removes the registry entry only (the real repo/dir is untouched).
  Immediate, no confirmation. Unknown slug -> `NotFound`.
- **rename**: re-slugs the entry, preserving kind/path/base_branch. Unknown ->
  `NotFound`; target taken -> `AlreadyExists`; empty -> `InvalidName`; same -> no-op.
- **ls**: sorted by slug; shows slug, kind, base_branch (git), path.

Duplicate slug on add -> `AlreadyExists`. Adding the same path under two slugs is
allowed (slug is the identity).

## Errors

New `InvalidSource { path, reason }` for "path does not exist", "not a
directory", "not a git repository". Reuses kind-tagged `NotFound`/`AlreadyExists`
("source"/"project") and `InvalidName`.

## Code

- New `src/source.rs`: `Source { slug(skip), kind, path, base_branch }`,
  `SourceKind { Git, Dir }`, and `list`, `add_git`, `add_dir`, `remove`,
  `rename`. Loads/saves the slug-keyed map; shells out to `git` for validation
  and base-branch detection.
- `store.rs`: `sources_path(project)`.
- `error.rs`: `InvalidSource`.
- `main.rs`: `Source` command (alias `src`) + global `--project`, fancy listing.

## Tests

Library (real fs in `TempDir`; real `git init` repos for git cases):
- add-dir: canonicalizes, default slug from folder, stored; missing path / a file
  -> `InvalidSource`; duplicate slug -> `AlreadyExists`; explicit slug.
- add-git: detects `main` and `master`; `--base-branch` override; non-git dir ->
  `InvalidSource`; default slug from repo folder.
- rm: removes entry; unknown -> `NotFound`; real dir left intact.
- rename: moves entry preserving fields; unknown -> `NotFound`; collision ->
  `AlreadyExists`.
- list: sorted by slug; empty with no file.

CLI (`assert_cmd`):
- add-dir then `ls` shows slug/kind/path; add-git shows branch; `-p` targets a
  project; rm; rename/mv; unknown errors.

## Out of scope

Materializing sources into workspace `src/` (worktrees, symlinks) — next
iteration. Cloning from a remote URL (only local paths here).
