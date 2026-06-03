# Spec — dirs (iteration 4)

## Goal

A flat set of folders inside each project. Two reserved folders, `main` and
`pinned`, always exist. Users can add / remove / rename any other folder.
Commands act on the default project unless `--project/-p <slug>` is given.

## Commands

```
mustr dir list                       # alias: d ;  list -> ls
mustr dir add <name>                 #             add  -> new   (name is slugified)
mustr dir rm <slug> [--yes]          #             rm   -> remove
mustr dir rename <slug> <new>        #             (rename = folder rename)

mustr dir rm abc -p cicero           # act on a specific project
mustr dir rm abc --project cicero
```

`--project/-p` is a global flag on `dir`, so it may appear after the subcommand.

## On-disk layout

```
~/.mustr/projects/<project>/
  project.toml
  main/      dir.toml      # { id, created_at }
  pinned/    dir.toml
  <slug>/    dir.toml
```

A dir is a subfolder containing `dir.toml`. Slug = folder name = identity
(derived, not stored), mirroring projects. Renaming a dir is a folder rename.

## Behavior

- **Reserved**: `main` and `pinned` cannot be added (already exist), removed, or
  renamed, nor can a dir be renamed *to* them -> `Reserved` error.
- **Default folders are guaranteed**: created when the project is created, and
  re-created on any `dir` access if deleted by hand (self-heal, like the project
  default).
- **Project resolution**: `-p` slug if given (must exist), else the resolved
  default project; if none -> error telling the user to create/select one.
- **add**: slugify `name`; empty -> `InvalidName`; reserved -> `Reserved`;
  exists -> `AlreadyExists`. Writes `dir.toml` atomically.
- **rm**: reserved -> `Reserved`; missing -> `NotFound`. TTY confirm; `--yes`
  skips (same as `project rm`).
- **rename**: reserved source/target -> `Reserved`; missing -> `NotFound`; empty
  new -> `InvalidName`; new already taken -> `AlreadyExists`; same slug -> no-op.
  `id`/`created_at` preserved.
- **list**: `main`, `pinned` first, then the rest by slug. Folders without
  `dir.toml` are ignored. Fancy output, age per dir.

## Errors

`NotFound` and `AlreadyExists` gain a `kind: &'static str` ("project" | "dir")
so messages read correctly for both. New `Reserved { slug }`.

## Code

- New `src/dir.rs`: `Dir { id, slug(skip), created_at }`, `add`, `list`,
  `remove`, `rename`, `ensure_defaults`. Checks project existence via the store
  (no dependency on `project.rs`).
- `project::add` calls `dir::ensure_defaults` so a new project ships with
  `main`/`pinned`.
- `store.rs`: `dir_path`, `dir_manifest_path` helpers.
- `main.rs`: `Dir` command + global `--project`, `resolve_project` helper, fancy
  dir listing.

## Tests

Library (real fs in `TempDir`, project created in setup):
- new project has `main` + `pinned`.
- add: creates folder + manifest; duplicate -> `AlreadyExists`; reserved ->
  `Reserved`; empty -> `InvalidName`; unknown project -> `NotFound`.
- list: `main`/`pinned` first then by slug; ignores stray entries.
- rm: deletes; reserved -> `Reserved`; missing -> `NotFound`.
- rename: moves + preserves identity; reserved source/target -> `Reserved`;
  collision -> `AlreadyExists`; missing -> `NotFound`; same slug no-op.
- `ensure_defaults` re-creates a hand-deleted `main`.

CLI (`assert_cmd` via `MUSTR_ROOT`):
- `dir list` shows `main`/`pinned`; `dir add` then `ls` shows it.
- `-p`/`--project` target a non-default project; both spellings work.
- `dir rm --yes`; `rm main` fails (reserved); `rm` without `--yes` non-TTY refuses.
- no project selected -> helpful error.

## Out of scope

Nested dirs, per-dir contents (worktrees/artifacts/sessions), a "current dir".
