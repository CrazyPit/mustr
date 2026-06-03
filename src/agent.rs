//! Coding-agent sessions in a workspace. mustr owns a stable per-(workspace,
//! slug) record, builds the right launch command per agent kind, and guards
//! double-launch with its own pid lock (agent-agnostic) since most agents have
//! no live-instance registry.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::slug::slugify;
use crate::store::{atomic_write, now_rfc3339, Store};

/// Kind of coding agent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AgentKind {
    Claude,
    Codex,
    Cursor,
}

impl AgentKind {
    /// Parses an agent kind name.
    pub fn parse(s: &str) -> Option<AgentKind> {
        match s {
            "claude" => Some(AgentKind::Claude),
            "codex" => Some(AgentKind::Codex),
            "cursor" => Some(AgentKind::Cursor),
            _ => None,
        }
    }

    /// Display name.
    pub fn as_str(self) -> &'static str {
        match self {
            AgentKind::Claude => "claude",
            AgentKind::Codex => "codex",
            AgentKind::Cursor => "cursor",
        }
    }
}

/// A persisted agent in a workspace's `agents/` dir.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Agent {
    /// Stable mustr id (uuid v7).
    pub id: String,
    /// Slug; the record filename, not stored in the file.
    #[serde(skip)]
    pub slug: String,
    pub kind: AgentKind,
    /// The agent's own session id once known (some kinds mint it on first run).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    pub created_at: String,
}

/// Loads the agent record for `slug`, creating it (kind `kind`, no session yet)
/// if absent.
pub fn resolve(
    store: &Store,
    project: &str,
    dir: &str,
    workspace: &str,
    kind: AgentKind,
    slug: &str,
) -> Result<Agent> {
    ensure_workspace(store, project, dir, workspace)?;
    let path = store.agent_manifest_path(project, dir, workspace, slug);
    if path.is_file() {
        return read_record(&path, slug);
    }
    let agent = Agent {
        id: uuid::Uuid::now_v7().to_string(),
        slug: slug.to_string(),
        kind,
        session_id: None,
        created_at: now_rfc3339(),
    };
    write_record(&path, &agent)?;
    Ok(agent)
}

/// Records the agent's session id (after minting or discovery).
pub fn set_session_id(
    store: &Store,
    project: &str,
    dir: &str,
    workspace: &str,
    slug: &str,
    session_id: &str,
) -> Result<()> {
    let path = store.agent_manifest_path(project, dir, workspace, slug);
    let mut agent = read_record(&path, slug)?;
    agent.session_id = Some(session_id.to_string());
    write_record(&path, &agent)
}

/// Lists a workspace's agents, sorted by slug.
pub fn list(store: &Store, project: &str, dir: &str, workspace: &str) -> Result<Vec<Agent>> {
    ensure_workspace(store, project, dir, workspace)?;
    let agents_dir = store.workspace_path(project, dir, workspace).join("agents");
    let entries = match std::fs::read_dir(&agents_dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(Error::io(&agents_dir, e)),
    };
    let mut agents = Vec::new();
    for entry in entries {
        let path = entry.map_err(|e| Error::io(&agents_dir, e))?.path();
        if path.extension().and_then(|e| e.to_str()) != Some("toml") {
            continue;
        }
        if let Some(slug) = path.file_stem().and_then(|s| s.to_str()) {
            agents.push(read_record(&path, slug)?);
        }
    }
    agents.sort_by(|a, b| a.slug.cmp(&b.slug));
    Ok(agents)
}

/// Removes an agent record. Its session transcript is untouched.
pub fn remove(store: &Store, project: &str, dir: &str, workspace: &str, slug: &str) -> Result<()> {
    ensure_workspace(store, project, dir, workspace)?;
    let path = store.agent_manifest_path(project, dir, workspace, slug);
    if !path.is_file() {
        return Err(Error::NotFound {
            kind: "agent",
            slug: slug.to_string(),
        });
    }
    std::fs::remove_file(&path).map_err(|e| Error::io(&path, e))?;
    let _ = std::fs::remove_file(store.agent_lock_path(project, dir, workspace, slug));
    Ok(())
}

/// Renames an agent record, preserving its id and session id.
pub fn rename(
    store: &Store,
    project: &str,
    dir: &str,
    workspace: &str,
    slug: &str,
    new: &str,
) -> Result<Agent> {
    ensure_workspace(store, project, dir, workspace)?;
    let path = store.agent_manifest_path(project, dir, workspace, slug);
    if !path.is_file() {
        return Err(Error::NotFound {
            kind: "agent",
            slug: slug.to_string(),
        });
    }
    let new_slug = slugify(new);
    if new_slug.is_empty() {
        return Err(Error::InvalidName {
            name: new.to_string(),
        });
    }
    if new_slug == slug {
        return read_record(&path, slug);
    }
    let new_path = store.agent_manifest_path(project, dir, workspace, &new_slug);
    if new_path.is_file() {
        return Err(Error::AlreadyExists {
            kind: "agent",
            slug: new_slug,
        });
    }
    std::fs::rename(&path, &new_path).map_err(|e| Error::io(&new_path, e))?;
    read_record(&new_path, &new_slug)
}

/// Builds the `(program, args)` to launch an agent. `resume` chooses restore vs
/// fresh; `session_id` is required except for a fresh Codex session.
pub fn command(kind: AgentKind, resume: bool, session_id: Option<&str>) -> (String, Vec<String>) {
    let id = || session_id.expect("session id required").to_string();
    let (program, args): (&str, Vec<String>) = match (kind, resume) {
        (AgentKind::Claude, true) => ("claude", vec!["--resume".into(), id()]),
        (AgentKind::Claude, false) => ("claude", vec!["--session-id".into(), id()]),
        (AgentKind::Cursor, _) => ("cursor-agent", vec!["--resume".into(), id()]),
        (AgentKind::Codex, true) => ("codex", vec!["resume".into(), id()]),
        (AgentKind::Codex, false) => ("codex", vec![]),
    };
    (program.to_string(), args)
}

/// Writes the run lock with the live child `pid`.
pub fn write_lock(
    store: &Store,
    project: &str,
    dir: &str,
    workspace: &str,
    slug: &str,
    pid: u32,
) -> Result<()> {
    atomic_write(
        &store.agent_lock_path(project, dir, workspace, slug),
        &pid.to_string(),
    )
}

/// Clears the run lock (idempotent).
pub fn clear_lock(store: &Store, project: &str, dir: &str, workspace: &str, slug: &str) {
    let _ = std::fs::remove_file(store.agent_lock_path(project, dir, workspace, slug));
}

/// The live pid holding the agent, if any. A lock whose pid is dead (or
/// unparseable) is stale: it is removed and reported as not running.
pub fn running(
    store: &Store,
    project: &str,
    dir: &str,
    workspace: &str,
    slug: &str,
    is_alive: impl Fn(u32) -> bool,
) -> Option<u32> {
    let path = store.agent_lock_path(project, dir, workspace, slug);
    let Ok(raw) = std::fs::read_to_string(&path) else {
        return None;
    };
    if let Some(pid) = raw.trim().parse::<u32>().ok().filter(|p| is_alive(*p)) {
        Some(pid)
    } else {
        let _ = std::fs::remove_file(&path);
        None
    }
}

/// Whether Claude has a transcript for `session_id` at `cwd`.
pub fn claude_transcript_exists(claude_home: &Path, cwd: &Path, session_id: &str) -> bool {
    claude_home
        .join("projects")
        .join(claude_path_slug(cwd))
        .join(format!("{session_id}.jsonl"))
        .is_file()
}

/// Claude Code's per-cwd project folder name: every non-alphanumeric character
/// becomes `-`.
pub fn claude_path_slug(path: &Path) -> String {
    path.to_string_lossy()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect()
}

/// Finds the newest Codex session id recorded for `cwd` under `codex_home`.
pub fn codex_discover(codex_home: &Path, cwd: &Path) -> Result<Option<String>> {
    let mut rollouts: Vec<(std::time::SystemTime, PathBuf)> = Vec::new();
    collect_jsonl(&codex_home.join("sessions"), &mut rollouts)?;
    rollouts.sort_by(|a, b| b.0.cmp(&a.0)); // newest first

    for (_, path) in rollouts {
        let Ok(raw) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Some(first) = raw.lines().next() else {
            continue;
        };
        let Ok(meta) = serde_json::from_str::<CodexMeta>(first) else {
            continue;
        };
        if meta.payload.cwd.as_deref() == cwd.to_str() {
            return Ok(meta.payload.id);
        }
    }
    Ok(None)
}

#[derive(Deserialize)]
struct CodexMeta {
    #[serde(default)]
    payload: CodexPayload,
}

#[derive(Default, Deserialize)]
struct CodexPayload {
    id: Option<String>,
    cwd: Option<String>,
}

fn collect_jsonl(dir: &Path, out: &mut Vec<(std::time::SystemTime, PathBuf)>) -> Result<()> {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(Error::io(dir, e)),
    };
    for entry in entries {
        let entry = entry.map_err(|e| Error::io(dir, e))?;
        let path = entry.path();
        let meta = entry.metadata().map_err(|e| Error::io(&path, e))?;
        if meta.is_dir() {
            collect_jsonl(&path, out)?;
        } else if path.extension().and_then(|e| e.to_str()) == Some("jsonl") {
            out.push((meta.modified().unwrap_or(std::time::UNIX_EPOCH), path));
        }
    }
    Ok(())
}

fn ensure_workspace(store: &Store, project: &str, dir: &str, workspace: &str) -> Result<()> {
    if store
        .workspace_manifest_path(project, dir, workspace)
        .is_file()
    {
        Ok(())
    } else {
        Err(Error::NotFound {
            kind: "workspace",
            slug: workspace.to_string(),
        })
    }
}

fn read_record(path: &Path, slug: &str) -> Result<Agent> {
    let raw = std::fs::read_to_string(path).map_err(|e| Error::io(path, e))?;
    let mut agent: Agent = toml::from_str(&raw).map_err(|source| Error::TomlRead {
        path: path.to_path_buf(),
        source,
    })?;
    agent.slug = slug.to_string();
    Ok(agent)
}

fn write_record(path: &Path, agent: &Agent) -> Result<()> {
    let raw = toml::to_string_pretty(agent).map_err(|source| Error::TomlWrite {
        path: path.to_path_buf(),
        source,
    })?;
    atomic_write(path, &raw)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup() -> (tempfile::TempDir, Store) {
        let tmp = tempfile::tempdir().unwrap();
        let store = Store::new(tmp.path());
        store.ensure().unwrap();
        crate::project::add(&store, "proj").unwrap();
        crate::workspace::add(&store, "proj", "main", "ws", None).unwrap();
        (tmp, store)
    }

    #[test]
    fn parse_and_as_str() {
        assert_eq!(AgentKind::parse("claude"), Some(AgentKind::Claude));
        assert_eq!(AgentKind::parse("codex"), Some(AgentKind::Codex));
        assert_eq!(AgentKind::parse("cursor"), Some(AgentKind::Cursor));
        assert_eq!(AgentKind::parse("opencode"), None);
        assert_eq!(AgentKind::Codex.as_str(), "codex");
    }

    #[test]
    fn command_per_kind() {
        assert_eq!(
            command(AgentKind::Claude, false, Some("s")),
            ("claude".into(), vec!["--session-id".into(), "s".into()])
        );
        assert_eq!(
            command(AgentKind::Claude, true, Some("s")),
            ("claude".into(), vec!["--resume".into(), "s".into()])
        );
        assert_eq!(
            command(AgentKind::Cursor, false, Some("c")),
            ("cursor-agent".into(), vec!["--resume".into(), "c".into()])
        );
        assert_eq!(
            command(AgentKind::Codex, false, None),
            ("codex".into(), Vec::<String>::new())
        );
        assert_eq!(
            command(AgentKind::Codex, true, Some("x")),
            ("codex".into(), vec!["resume".into(), "x".into()])
        );
    }

    #[test]
    fn resolve_creates_record_without_session_id() {
        let (_tmp, store) = setup();
        let a = resolve(&store, "proj", "main", "ws", AgentKind::Codex, "main").unwrap();
        assert_eq!(a.kind, AgentKind::Codex);
        assert_eq!(a.session_id, None);

        set_session_id(&store, "proj", "main", "ws", "main", "sid").unwrap();
        let b = resolve(&store, "proj", "main", "ws", AgentKind::Codex, "main").unwrap();
        assert_eq!(b.session_id.as_deref(), Some("sid"));
    }

    #[test]
    fn running_reflects_lock_and_liveness() {
        let (_tmp, store) = setup();
        assert_eq!(
            running(&store, "proj", "main", "ws", "main", |_| true),
            None
        );

        write_lock(&store, "proj", "main", "ws", "main", 4242).unwrap();
        assert_eq!(
            running(&store, "proj", "main", "ws", "main", |p| p == 4242),
            Some(4242)
        );
        // Dead pid -> stale -> removed -> not running.
        assert_eq!(
            running(&store, "proj", "main", "ws", "main", |_| false),
            None
        );
        assert!(!store
            .agent_lock_path("proj", "main", "ws", "main")
            .is_file());

        clear_lock(&store, "proj", "main", "ws", "main");
        assert_eq!(
            running(&store, "proj", "main", "ws", "main", |_| true),
            None
        );
    }

    #[test]
    fn claude_transcript_exists_checks_cwd_slug_path() {
        let tmp = tempfile::tempdir().unwrap();
        let cwd = Path::new("/ws");
        let proj = tmp.path().join("projects").join(claude_path_slug(cwd));
        std::fs::create_dir_all(&proj).unwrap();
        assert!(!claude_transcript_exists(tmp.path(), cwd, "sid"));
        std::fs::write(proj.join("sid.jsonl"), "{}").unwrap();
        assert!(claude_transcript_exists(tmp.path(), cwd, "sid"));
    }

    #[test]
    fn codex_discover_finds_newest_session_for_cwd() {
        let tmp = tempfile::tempdir().unwrap();
        let day = tmp.path().join("sessions/2026/06/03");
        std::fs::create_dir_all(&day).unwrap();
        std::fs::write(
            day.join("rollout-a.jsonl"),
            r#"{"type":"session_meta","payload":{"id":"id-other","cwd":"/elsewhere"}}"#,
        )
        .unwrap();
        std::fs::write(
            day.join("rollout-b.jsonl"),
            r#"{"type":"session_meta","payload":{"id":"id-mine","cwd":"/ws"}}"#,
        )
        .unwrap();

        assert_eq!(
            codex_discover(tmp.path(), Path::new("/ws"))
                .unwrap()
                .as_deref(),
            Some("id-mine")
        );
        assert_eq!(
            codex_discover(tmp.path(), Path::new("/nope")).unwrap(),
            None
        );
    }

    #[test]
    fn list_remove_rename() {
        let (_tmp, store) = setup();
        resolve(&store, "proj", "main", "ws", AgentKind::Claude, "main").unwrap();
        resolve(&store, "proj", "main", "ws", AgentKind::Codex, "review").unwrap();

        let slugs: Vec<_> = list(&store, "proj", "main", "ws")
            .unwrap()
            .into_iter()
            .map(|a| a.slug)
            .collect();
        assert_eq!(slugs, vec!["main", "review"]);

        let renamed = rename(&store, "proj", "main", "ws", "review", "audit").unwrap();
        assert_eq!(renamed.slug, "audit");
        assert_eq!(renamed.kind, AgentKind::Codex);

        remove(&store, "proj", "main", "ws", "main").unwrap();
        assert!(matches!(
            remove(&store, "proj", "main", "ws", "main"),
            Err(Error::NotFound { kind: "agent", .. })
        ));
    }

    #[test]
    fn rename_collision_errors() {
        let (_tmp, store) = setup();
        resolve(&store, "proj", "main", "ws", AgentKind::Claude, "a").unwrap();
        resolve(&store, "proj", "main", "ws", AgentKind::Claude, "b").unwrap();
        assert!(matches!(
            rename(&store, "proj", "main", "ws", "a", "b"),
            Err(Error::AlreadyExists { kind: "agent", .. })
        ));
    }

    #[test]
    fn resolve_unknown_workspace_errors() {
        let (_tmp, store) = setup();
        assert!(matches!(
            resolve(&store, "proj", "main", "ghost", AgentKind::Claude, "main"),
            Err(Error::NotFound {
                kind: "workspace",
                ..
            })
        ));
    }
}
