# Repository Instructions

## Feature Development

Every feature, behavior change, or bug fix must treat tests as part of the work, not as a follow-up.

- Add new tests for new behavior.
- Update existing tests whenever changed behavior makes old expectations incomplete or outdated.
- Keep coverage as complete as reasonably possible, including happy paths, edge cases, and failure paths.
- Run the relevant test suite before considering the feature complete.
- A feature is not done while tests are failing.
- Commit the work for each feature once it is complete and tests pass.

Preferred test command:

```bash
cargo test
```

## Runtime Data Format

Persistent data the app writes under `~/.mustr/` splits by shape:

- **Metadata and config** (project metadata, workspace metadata, session-manifest descriptions, config) is **TOML**. Human-readable and hand-editable. If the user asks for a "yaml file", a "config file", or a "manifest" in this context, use TOML — regardless of the extension they name. Write atomically (temp file + rename).
- **Append-only streams** (agent-session transcripts, terminal scrollback — anything written incrementally over time) is **line-based**: JSONL for structured frames, raw bytes otherwise. TOML cannot be appended to, so it must not be used here.

Inside the project tree (Cargo.toml, etc.) use whatever format the tool/framework expects.
