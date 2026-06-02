# mustr

Command center for coding-agent work — a CLI for organizing projects,
worktrees, and agent sessions. Data lives under `~/.mustr/`.

Early work in progress. Current surface: `mustr project add | rm | rename | list`
(alias `p`).

## Develop

```bash
cargo test        # run the suite
cargo run -- p ls # run from source
```

## Install

Runs the tests, builds a release binary, and copies it to `~/bin/`:

```bash
cargo xtask install
```

Make sure `~/bin` is on your `PATH`.
