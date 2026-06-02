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

All persistent data the app writes under `~/.mustr/` (project metadata, workspace metadata, agent-session manifests, per-session artifacts — everything under the user's data root) is JSON. If the user asks for a "yaml file", a "config file", or a "manifest" in that context, use JSON — regardless of the extension they name. Write atomically (temp file + rename).

Inside the project tree (Cargo.toml, etc.) use whatever format the tool/framework expects — don't force JSON there.
