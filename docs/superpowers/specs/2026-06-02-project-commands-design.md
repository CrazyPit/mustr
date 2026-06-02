# Spec — `project` commands (iteration 1)

## Goal

1. Calling any command initializes `~/.mustr` and everything it needs.
2. Every command works with its alias and without it.
3. Good tests with minimal mocks for everything.
4. Code follows Rust best practices.

## Scope

CRUD over *projects* — named container folders under `~/.mustr/projects/`. No repos,
worktrees, or agent sessions yet; those are later iterations.

```
mustr project add <name>            # alias: p ;  add  -> alias: new
mustr project rm <slug> [--yes]     # alias: p ;  rm   -> alias: remove
mustr project rename <slug> <name>  # alias: p
mustr project list                  # alias: p ;  list -> alias: ls
```

## Architecture

Library crate + thin binary, so a future GUI reuses the same library and tests hit the
logic directly.

```
mustr/
  Cargo.toml
  src/
    lib.rs        # re-exports the public API
    store.rs      # Store { root: PathBuf }; ensure(); path helpers
    slug.rs       # slugify(name) -> String
    project.rs    # Project + add / remove / rename / list
    config.rs     # Config { default_project: Option<String> } at ~/.mustr/config.toml
    error.rs      # thiserror Error
    main.rs       # clap (derive) parsing + output rendering
```

**Injectable root.** `Store::new(root)`. The binary resolves the root from the
`MUSTR_ROOT` env var if set, otherwise `~/.mustr`. This makes CLI tests hermetic
(point `MUSTR_ROOT` at a temp dir) with no trait mocks — "minimal mocks" means the real
filesystem inside a `tempfile::TempDir`.

**Auto-init.** Every command first calls `store.ensure()` (`create_dir_all` for
`~/.mustr` and `projects/`, idempotent). Satisfies goal 1 uniformly.

## Dependencies

`clap` (derive), `serde` (derive), `toml`, `uuid` (v7), `time`, `thiserror`,
`owo-colors`. Dev: `tempfile`, `assert_cmd`.

## Data format

Metadata is TOML (per AGENTS.md). `projects/<slug>/project.toml`:

```toml
id = "0192f0a1-...-7c3d"          # uuid v7
name = "Fix login"
slug = "fix-login"
created_at = "2026-06-02T11:00:00Z"   # RFC3339
```

`~/.mustr/config.toml`:

```toml
default_project = "fix-login"     # optional
```

All metadata writes are atomic (temp file + rename).

## Behavior

- **add**: `slugify(name)`. Empty name / empty slug -> `InvalidName`. Existing
  `projects/<slug>/` -> `AlreadyExists`. Writes `project.toml` atomically. If no default
  is set yet, this project becomes the default.
- **rm**: unknown slug -> `NotFound`. In a TTY, prompt `Delete project '<slug>'? [y/N]`;
  `--yes` skips the prompt. If the removed project was the default, the default moves to
  the first remaining project (alphabetical by slug), or `None` if none remain.
- **rename**: re-slug from the new name. Same resulting slug -> only update `name`. New
  slug already taken -> `AlreadyExists`. `id` and `created_at` are preserved; if the
  renamed project was the default, the default follows the new slug.
- **list**: read every `projects/*/project.toml`, sort by name, mark the default with `★`.

## Slug rules

Lowercase; non-alphanumeric runs collapse to a single `-`; trim leading/trailing `-`;
truncate to a max length then re-trim edges. Empty result is rejected by the caller as
`InvalidName`.

## `list` output

Clean aligned columns, dimmed secondary text, accent `★` for the default, `NO_COLOR`
respected. No box borders.

```
  projects · ~/.mustr/projects

  ★ Fix login        fix-login        2 days ago
    Metrics watch    metrics-watch    5 days ago
    Personal         personal         3 weeks ago

  3 projects · ★ default
```

Empty list -> friendly empty state hinting `mustr project add <name>`.

## Errors & exit codes

`error.rs` defines a `thiserror` enum: `NotFound`, `AlreadyExists`, `InvalidName`,
`Io`, `Toml`. The library returns `Result<_, Error>`; `main` prints the error to stderr
and exits non-zero.

## Tests

Library (unit, real fs in `TempDir`):
- slugify: case, spaces, unicode, punctuation, empty, truncation.
- add: creates folder + `project.toml` with correct fields; duplicate slug -> `AlreadyExists`; first add sets default.
- rename: folder move + metadata update; same-slug no-op move; collision -> `AlreadyExists`; `id`/`created_at` preserved; default follows.
- rm: removes folder; unknown -> `NotFound`; default reassignment.
- list: name sort; ignores stray files/dirs without `project.toml`.
- config: read / write / default round-trip.

CLI (`assert_cmd` via `MUSTR_ROOT`):
- `project` and `p` produce identical results (goal 2).
- first command creates `~/.mustr` (goal 1).
- `rm --yes` deletes without prompting.
- exit codes and stderr on error paths.

## Out of scope (next iterations)

Switching the default project (non-interactive command + interactive picker), repos,
worktrees, agent sessions.
