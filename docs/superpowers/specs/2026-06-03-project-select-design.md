# Spec — project selection (iteration 3)

## Goal

Choose the active (default) project explicitly, and self-heal the default when
the project it points at disappears (removed by `mustr` or deleted by hand).

## Command

```
mustr project default <slug>   # aliases: take, select ;  project alias: p
mustr project default          # no arg -> interactive picker (TTY only)
```

## Behavior

- **`default <slug>`**: unknown slug -> `NotFound`. Sets `default_project = slug`,
  saves config. Prints the project directory path to **stdout** (only the path).
  A short confirmation goes to **stderr**.
- **`default` (no arg)**: if stdout/stderr is not a TTY -> error (cannot pick
  interactively). If there are no projects -> error with a hint. Otherwise show
  an arrow-key picker (rendered on stderr); the chosen project becomes the
  default and its path prints to stdout.

### stdout / stderr contract

The path is the **only** thing on stdout, so a shell wrapper works:

```sh
mcd() { cd "$(mustr p default "$@")"; }
```

Everything else — confirmations, the picker UI, errors — goes to stderr.

## Self-healing default (lazy, on read)

A single function owns "what is the effective default":

`project::resolve_default(store) -> Result<Option<String>>`
- Load config and the project list (sorted by name).
- If `default_project` names an existing project, return it unchanged.
- Otherwise (dangling, or `None` while projects exist) pick the first project by
  name, persist it to config, and return it.
- If there are no projects, set `None` (persist if it changed) and return `None`.

This handles a default deleted by `mustr` **and** one deleted by hand, the same
way, on the next command that reads it.

## Refactor (unify existing default logic)

- `add` and `remove` stop hand-rolling default assignment; they create/delete and
  then call `resolve_default`, which already does the right thing. This also
  aligns reassignment ordering to "first by name" (matching `list`); the existing
  `remove_default_reassigns_then_clears` test stays green.
- New `project::set_default(store, slug)` validates the slug exists and persists
  it; `default` uses it.
- `list` rendering uses `resolve_default` so the `★` marker is correct even after
  a manual deletion.

## Dependencies

`dialoguer` for the arrow-key picker (renders on stderr).

## Tests

Library (real fs in `TempDir`):
- `set_default`: persists; unknown slug -> `NotFound`.
- `resolve_default`: valid default stays; dangling default (folder removed by
  hand) heals to first-by-name and persists; `None` with projects heals to first;
  no projects -> `None`.
- `add` first project becomes default; `remove` of default reassigns / clears
  (existing tests remain green after the refactor).

CLI (`assert_cmd` via `MUSTR_ROOT`):
- `default <slug>` moves the `★` in `list`.
- `default <slug>` prints the project path (and only the path) on stdout.
- `default` unknown slug fails with "not found" on stderr.
- manual deletion of the default folder, then `list`, shows `★` healed onto
  another project.
- (the interactive picker is TTY-only and verified by hand, not in `assert_cmd`.)

## Plan

1. `resolve_default` (TDD); refactor `add`/`remove` to delegate.
2. `set_default` (TDD).
3. CLI `default`/`take`/`select` subcommand: stdout=path, stderr=confirmation.
4. Interactive picker (dialoguer) for the no-arg case.
5. README shell-wrapper note.
6. `cargo fmt` / `clippy` / `cargo test`, then `cargo xtask install`.

## Out of scope

Actual repos/worktrees inside a project (later); fuzzy matching of names.
