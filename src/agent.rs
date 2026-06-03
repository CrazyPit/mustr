//! Opening coding-agent sessions in a workspace. Currently Claude Code: we own a
//! stable session id per (workspace, agent slug), launch with the right flag,
//! and refuse to open a session that is already running.

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
}

/// A persisted agent session in a workspace's `agents/` dir.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Agent {
    /// Stable mustr id (uuid v7).
    pub id: String,
    /// Slug; the agent record filename, not stored in the file.
    #[serde(skip)]
    pub slug: String,
    pub kind: AgentKind,
    /// The coding agent's own session id (we generate and pin it).
    pub session_id: String,
    pub created_at: String,
}

/// What [`plan`] decided to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpenPlan {
    /// A live process already holds this session.
    AlreadyRunning { pid: u32 },
    /// Launch the agent with these args, in `cwd`. `resume` is false for a fresh
    /// session, true when restoring an existing one.
    Launch {
        args: Vec<String>,
        cwd: PathBuf,
        resume: bool,
    },
}

/// Loads the agent record for `slug`, creating it (with a fresh session id) if
/// absent.
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
        session_id: uuid::Uuid::now_v7().to_string(),
        created_at: now_rfc3339(),
    };
    write_record(&path, &agent)?;
    Ok(agent)
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

/// Removes an agent record. The agent's own session transcript is untouched.
pub fn remove(store: &Store, project: &str, dir: &str, workspace: &str, slug: &str) -> Result<()> {
    ensure_workspace(store, project, dir, workspace)?;
    let path = store.agent_manifest_path(project, dir, workspace, slug);
    if !path.is_file() {
        return Err(Error::NotFound {
            kind: "agent",
            slug: slug.to_string(),
        });
    }
    std::fs::remove_file(&path).map_err(|e| Error::io(&path, e))
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

/// Decides how to open `agent` whose workspace is at `cwd`, consulting Claude's
/// state under `claude_home`. `is_alive` reports whether a pid is running.
pub fn plan(
    agent: &Agent,
    cwd: &Path,
    claude_home: &Path,
    is_alive: impl Fn(u32) -> bool,
) -> Result<OpenPlan> {
    if let Some(pid) = running_pid(claude_home, &agent.session_id, is_alive)? {
        return Ok(OpenPlan::AlreadyRunning { pid });
    }

    let transcript = claude_home
        .join("projects")
        .join(claude_path_slug(cwd))
        .join(format!("{}.jsonl", agent.session_id));
    let resume = transcript.is_file();
    let flag = if resume { "--resume" } else { "--session-id" };
    Ok(OpenPlan::Launch {
        args: vec![flag.to_string(), agent.session_id.clone()],
        cwd: cwd.to_path_buf(),
        resume,
    })
}

/// Claude Code's per-cwd project folder name: every non-alphanumeric character
/// becomes `-` (no collapsing).
pub fn claude_path_slug(path: &Path) -> String {
    path.to_string_lossy()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect()
}

/// Scans `claude_home/sessions/*.json` for a live process holding `session_id`.
pub fn running_pid(
    claude_home: &Path,
    session_id: &str,
    is_alive: impl Fn(u32) -> bool,
) -> Result<Option<u32>> {
    #[derive(Deserialize)]
    struct Entry {
        pid: u32,
        #[serde(rename = "sessionId")]
        session_id: String,
    }

    let dir = claude_home.join("sessions");
    let entries = match std::fs::read_dir(&dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(Error::io(&dir, e)),
    };
    for entry in entries {
        let path = entry.map_err(|e| Error::io(&dir, e))?.path();
        let Ok(raw) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Ok(reg) = serde_json::from_str::<Entry>(&raw) else {
            continue;
        };
        if reg.session_id == session_id && is_alive(reg.pid) {
            return Ok(Some(reg.pid));
        }
    }
    Ok(None)
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

    fn agent(session_id: &str) -> Agent {
        Agent {
            id: "id".into(),
            slug: "main".into(),
            kind: AgentKind::Claude,
            session_id: session_id.into(),
            created_at: "2026-06-03T00:00:00Z".into(),
        }
    }

    #[test]
    fn path_slug_replaces_non_alphanumerics() {
        assert_eq!(
            claude_path_slug(Path::new("/Users/cpwork/.claude")),
            "-Users-cpwork--claude"
        );
        assert_eq!(claude_path_slug(Path::new("/a/b-c")), "-a-b-c");
    }

    #[test]
    fn resolve_creates_and_persists_a_record() {
        let (_tmp, store) = setup();

        let a = resolve(&store, "proj", "main", "ws", AgentKind::Claude, "main").unwrap();
        assert_eq!(a.slug, "main");
        assert_eq!(a.kind, AgentKind::Claude);
        assert!(!a.session_id.is_empty());
        assert!(store
            .agent_manifest_path("proj", "main", "ws", "main")
            .is_file());

        // Second resolve returns the same session id.
        let b = resolve(&store, "proj", "main", "ws", AgentKind::Claude, "main").unwrap();
        assert_eq!(b.session_id, a.session_id);
    }

    #[test]
    fn list_returns_agents_sorted_by_slug() {
        let (_tmp, store) = setup();
        resolve(&store, "proj", "main", "ws", AgentKind::Claude, "review").unwrap();
        resolve(&store, "proj", "main", "ws", AgentKind::Claude, "main").unwrap();

        let slugs: Vec<_> = list(&store, "proj", "main", "ws")
            .unwrap()
            .into_iter()
            .map(|a| a.slug)
            .collect();
        assert_eq!(slugs, vec!["main", "review"]);
    }

    #[test]
    fn list_empty_when_no_agents() {
        let (_tmp, store) = setup();
        assert!(list(&store, "proj", "main", "ws").unwrap().is_empty());
    }

    #[test]
    fn remove_deletes_record() {
        let (_tmp, store) = setup();
        resolve(&store, "proj", "main", "ws", AgentKind::Claude, "main").unwrap();

        remove(&store, "proj", "main", "ws", "main").unwrap();

        assert!(!store
            .agent_manifest_path("proj", "main", "ws", "main")
            .is_file());
        assert!(matches!(
            remove(&store, "proj", "main", "ws", "main"),
            Err(Error::NotFound { kind: "agent", .. })
        ));
    }

    #[test]
    fn rename_moves_record_preserving_session_id() {
        let (_tmp, store) = setup();
        let a = resolve(&store, "proj", "main", "ws", AgentKind::Claude, "main").unwrap();

        let renamed = rename(&store, "proj", "main", "ws", "main", "review").unwrap();

        assert_eq!(renamed.slug, "review");
        assert_eq!(renamed.session_id, a.session_id);
        assert_eq!(renamed.id, a.id);
        assert!(!store
            .agent_manifest_path("proj", "main", "ws", "main")
            .is_file());
    }

    #[test]
    fn rename_collision_errors() {
        let (_tmp, store) = setup();
        resolve(&store, "proj", "main", "ws", AgentKind::Claude, "main").unwrap();
        resolve(&store, "proj", "main", "ws", AgentKind::Claude, "review").unwrap();
        assert!(matches!(
            rename(&store, "proj", "main", "ws", "main", "review"),
            Err(Error::AlreadyExists { kind: "agent", .. })
        ));
    }

    #[test]
    fn rename_unknown_errors() {
        let (_tmp, store) = setup();
        assert!(matches!(
            rename(&store, "proj", "main", "ws", "ghost", "x"),
            Err(Error::NotFound { kind: "agent", .. })
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

    #[test]
    fn plan_starts_fresh_when_no_transcript() {
        let tmp = tempfile::tempdir().unwrap();
        let plan = plan(&agent("sid-1"), Path::new("/ws"), tmp.path(), |_| false).unwrap();
        assert_eq!(
            plan,
            OpenPlan::Launch {
                args: vec!["--session-id".into(), "sid-1".into()],
                cwd: PathBuf::from("/ws"),
                resume: false,
            }
        );
    }

    #[test]
    fn plan_resumes_when_transcript_exists() {
        let tmp = tempfile::tempdir().unwrap();
        let cwd = Path::new("/ws");
        let proj = tmp.path().join("projects").join(claude_path_slug(cwd));
        std::fs::create_dir_all(&proj).unwrap();
        std::fs::write(proj.join("sid-1.jsonl"), "{}").unwrap();

        let plan = plan(&agent("sid-1"), cwd, tmp.path(), |_| false).unwrap();
        assert_eq!(
            plan,
            OpenPlan::Launch {
                args: vec!["--resume".into(), "sid-1".into()],
                cwd: cwd.to_path_buf(),
                resume: true,
            }
        );
    }

    #[test]
    fn plan_reports_already_running_for_live_session() {
        let tmp = tempfile::tempdir().unwrap();
        let sessions = tmp.path().join("sessions");
        std::fs::create_dir_all(&sessions).unwrap();
        std::fs::write(
            sessions.join("123.json"),
            r#"{"pid":123,"sessionId":"sid-1","cwd":"/ws"}"#,
        )
        .unwrap();

        let plan = plan(&agent("sid-1"), Path::new("/ws"), tmp.path(), |p| p == 123).unwrap();
        assert_eq!(plan, OpenPlan::AlreadyRunning { pid: 123 });
    }

    #[test]
    fn plan_ignores_dead_pid_in_registry() {
        let tmp = tempfile::tempdir().unwrap();
        let sessions = tmp.path().join("sessions");
        std::fs::create_dir_all(&sessions).unwrap();
        std::fs::write(
            sessions.join("123.json"),
            r#"{"pid":123,"sessionId":"sid-1","cwd":"/ws"}"#,
        )
        .unwrap();

        // pid is not alive -> not a conflict.
        let plan = plan(&agent("sid-1"), Path::new("/ws"), tmp.path(), |_| false).unwrap();
        assert!(matches!(plan, OpenPlan::Launch { .. }));
    }
}
