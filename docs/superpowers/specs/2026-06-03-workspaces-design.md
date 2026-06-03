# Spec — workspaces (iteration 5)

## Goal

Workspaces live inside a project's dirs: `~/.mustr/projects/<p>/<dir>/<ws>/`.
A workspace has a slug (identity), an optional description, and a created time.
`rm` is a soft delete into the `trash` dir; permanent deletes are explicit.

## Address

`[dir/]slug` — dir defaults to `main`. Examples: `tb-123` (= `main/tb-123`),
`pinned/abcd`, `trash/tb-123`. The slug part is slugified; the dir must exist.
`--project/-p` selects the project (default project otherwise), as for `dir`.

## Commands (`w` = `workspace`)

```
mustr w add [dir/]slug [-d "desc"]      # new ; description optional
mustr w rm  [dir/]slug [-f] [-y]        # soft-delete -> trash; -f/--force = permanent
mustr w purge [-y]                      # empty the trash dir (permanent)
mustr w rename [dir/]slug [new] [-d d]  # rename slug and/or set description
mustr w mv  [dir/]slug <target-dir>     # move workspace to another dir
mustr w list [dir]                      # ls ; all dirs (prefixed) or one dir (no prefix)
mustr w grep <query>                    # search slug+description, case-insensitive
```

`mv` here is a real move between dirs (not a rename alias). `rename` requires at
least one of `new` / `-d`.

## Data

`workspace.toml`: `{ id, created_at, description? }`. slug = folder name
(derived). `description` omitted when unset.

## Behavior

- **add**: project+dir must exist; slugify; empty -> `InvalidName`; taken ->
  `AlreadyExists` (kind "workspace").
- **rm**:
  - target in `trash`, or `--force`: permanent (`remove_dir_all`).
  - otherwise: move the folder into `trash/`. Reversible.
  - On a name clash in the destination, auto-suffix (`slug-2`, `slug-3`, …) and
    report the chosen name.
- **purge**: delete every workspace in `trash`; report the count.
- **rename**: `new` -> rename folder within the same dir (clash -> `AlreadyExists`,
  no auto-suffix — the name was explicit). `-d` -> set/replace description.
- **mv**: move folder to `<target-dir>` (must exist). Same-dir is a no-op.
  Clash in target -> auto-suffix with a report. `mv x trash` is allowed.
- **list**: no dir -> every dir, rows prefixed `dir/slug`; a dir arg -> just that
  dir, rows unprefixed. Order: reserved dirs first then by slug, then slug.
- **grep**: across all dirs, case-insensitive substring in slug or description;
  output prefixed like `list` (all dirs).

## Confirmation (only permanent deletes)

Soft `rm` -> `trash` is silent (reversible). Permanent ones — `rm --force`,
`rm trash/x`, `purge` — prompt on a TTY and require `--yes` otherwise (reusing
`confirm_removal`).

## Auto-suffix notification

Whenever a move auto-suffixes (rm->trash, mv), the command reports the final name,
e.g. `Moved tb-123 to trash as tb-123-2 (name was taken)`.

## Code

- New `src/workspace.rs`: `Workspace { id, slug(skip), dir(skip), created_at,
  description }`, plus `add`, `remove`, `purge`, `rename`, `move_to_dir`, `list`,
  `grep`, and `parse_address`. Validates project/dir via the store.
- `store.rs`: `workspace_path`, `workspace_manifest_path`.
- `main.rs`: `Workspace` command (alias `w`) + global `--project`, address
  parsing, fancy listing with optional dir prefix + description + age.

## Tests

Library (real fs in `TempDir`, project+dirs in setup):
- add: creates folder + manifest (with/without description); duplicate ->
  `AlreadyExists`; unknown dir/project -> `NotFound`; empty -> `InvalidName`.
- parse_address: `slug` -> (main, slug); `dir/slug` -> (dir, slug); slugifies.
- rm soft: moves into trash; auto-suffix on clash returns the new slug; `--force`
  and in-trash delete permanently.
- purge: removes all of trash, returns count; empty trash -> 0.
- rename: slug rename moves folder, preserves id/created_at; clash ->
  `AlreadyExists`; `-d` sets description; both together.
- mv: moves to target dir; same-dir no-op; clash auto-suffix; unknown target ->
  `NotFound`.
- list: single dir sorted by slug; all-dirs includes dir on each; ignores stray.
- grep: matches slug and description, case-insensitive; no match -> empty.

CLI (`assert_cmd` via `MUSTR_ROOT`):
- add then `ls` shows it with description; `ls main` unprefixed, `ls` prefixed.
- `rm` soft then `ls trash` shows it; `rm trash/x -y` removes it; `rm x -f -y`
  permanent.
- `mv x pinned` then `ls pinned` shows it.
- `rename x y` and `rename x -d "..."`.
- `grep` finds by description; output prefixed.
- `purge -y` empties trash.
- collision path prints the auto-suffixed name.

## Out of scope

Workspace contents (worktrees/artifacts/agent sessions); nested dirs; restoring
from trash by name (use `mv trash/x main`).
