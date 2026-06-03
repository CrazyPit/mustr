# Spec — opening agents (iteration 8, Claude Code)

## Research: how Claude Code stores sessions (v2.1.156)

- **Transcripts**: `~/.claude/projects/<cwd-slug>/<session-id>.jsonl` (+ a
  `<session-id>/` sidecar with `subagents/`, `tool-results/`). `<cwd-slug>` is
  the absolute cwd with every non-alphanumeric char replaced by `-` (no
  collapsing: `/Users/x/.claude` → `-Users-x--claude`). Also
  `~/.claude/todos/<id>*`, `session-env/<id>/`, `debug/<id>.txt`.
  → Sessions are already isolated **per cwd**, so two workspaces never collide.
- **Live-instance registry**: `~/.claude/sessions/<pid>.json` =
  `{ pid, sessionId, cwd, status: idle|busy, kind, version, updatedAt }`,
  written while a process runs. This is the conflict-detection source of truth.
- **Flags**: `--session-id <uuid>` starts a *new* session with a pinned id
  (errors "Session ID already in use" if taken); `-r/--resume [id]` restores
  (can mint a new id, can fail on stale metadata); `-c/--continue`,
  `--fork-session`, `-n/--name`.
- **`CLAUDE_CONFIG_DIR`** overrides `~/.claude`.

Sources: code.claude.com/docs/sessions, anthropics/claude-code issues #5524,
#12235, #33912.

## Model

Per workspace, an agent record at `agents/<slug>.toml`:

```toml
id = "0192…"          # mustr id (uuid v7)
kind = "claude"
session_id = "0192…"  # the agent's own session id; we generate and pin it
created_at = "2026-06-03T…Z"
```

`mustr agent open claude` uses slug `main`; `mustr agent open claude <slug>` runs
another of the same kind (distinct `session_id`, same cwd). Workspace comes from
cwd or `-w/-p`; the agent runs with cwd = the workspace root.

## Behavior

1. `resolve` loads/creates `agents/<slug>.toml` (fresh `session_id` if new).
2. `plan` consults `claude_home`:
   - a live registry entry (`sessions/*.json`) with our `session_id` and an alive
     pid → **AlreadyRunning { pid }** → the CLI alerts and exits non-zero.
   - else the transcript `projects/<cwd-slug>/<session_id>.jsonl` exists →
     **Launch `--resume <id>`** (restore); otherwise **Launch `--session-id <id>`**
     (fresh, pinned).
3. The CLI `exec`s `claude` with those args in the workspace cwd (replacing the
   process, inheriting the TTY).

Conflict matching is by `session_id` (not cwd) so multiple slugs in one workspace
don't false-conflict. Liveness is `kill -0`.

## Code

- `agent.rs`: `Agent`, `AgentKind::Claude`, `OpenPlan`, `resolve`, `plan`,
  `claude_path_slug`; registry scan via `serde_json`. Pure/​injectable
  (`claude_home`, `is_alive`) so it's unit-tested without spawning.
- `store::agent_manifest_path`; `main.rs` `Agent` command (alias `a`), `claude_home`,
  `process_alive`, `exec`.

## Known caveats (to revisit)

- `--resume` can change the session id → our pinned id drifts. MVP keeps the
  stored id; a later pass can re-capture from the registry/cwd after launch.
- Stale "in use" after a crash (dead pid lingering) → we treat dead-pid entries
  as not running; cleaning orphaned `<id>.*` before `--resume` is a later option.
- We don't relocate transcripts into the workspace — Claude owns `~/.claude`; we
  only pin a stable `session_id` and guard double-launch.

## Out of scope

Other agents (Codex, cursor-agent) — same mechanism later; listing/closing
agents; capturing/streaming transcripts into `docs/`.
