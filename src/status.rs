//! A single-screen overview of the current context, derived from the cwd. The
//! data is assembled here (so a future GUI can reuse it); the CLI renders it.

use std::path::PathBuf;

use crate::context::Context;
use crate::error::Result;
use crate::store::Store;
use crate::{agent, dir, mount, project, workspace};

/// The overview, scoped to wherever the cwd is.
pub enum Status {
    /// Outside any project: every project with rollup counts.
    Global(GlobalStatus),
    /// Inside a project but not a workspace: its dirs and workspaces.
    Project(ProjectStatus),
    /// Inside a workspace: its sources, agents, and artifacts.
    Workspace(WorkspaceStatus),
}

pub struct GlobalStatus {
    pub projects: Vec<ProjectStat>,
}

pub struct ProjectStat {
    pub slug: String,
    pub workspaces: usize,
    pub agents: usize,
    pub running: usize,
}

pub struct ProjectStatus {
    pub project: String,
    /// Non-trash dirs, each with its workspaces.
    pub dirs: Vec<DirGroup>,
    /// Trash is summarized by count only.
    pub trash: usize,
    pub agents_total: usize,
    pub agents_running: usize,
}

pub struct DirGroup {
    pub dir: String,
    pub workspaces: Vec<WsRow>,
}

pub struct WsRow {
    pub slug: String,
    pub description: Option<String>,
}

pub struct WorkspaceStatus {
    pub project: String,
    pub dir: String,
    pub slug: String,
    pub description: Option<String>,
    pub sources: Vec<SourceRow>,
    pub agents: Vec<AgentRow>,
    pub artifacts: Vec<String>,
    pub path: PathBuf,
}

pub struct SourceRow {
    pub slug: String,
    pub kind: &'static str,
    pub detail: String,
}

pub struct AgentRow {
    pub slug: String,
    pub kind: &'static str,
    /// The live pid holding the agent, if it is running.
    pub running: Option<u32>,
}

/// Builds the overview for `ctx`. `is_alive` decides whether a locked pid is a
/// live process (injected so the build stays testable).
pub fn build(store: &Store, ctx: &Context, is_alive: impl Fn(u32) -> bool) -> Result<Status> {
    match (&ctx.project, &ctx.dir, &ctx.workspace) {
        (Some(project), Some(dir), Some(slug)) => {
            build_workspace(store, project, dir, slug, &is_alive).map(Status::Workspace)
        }
        (Some(project), _, _) => build_project(store, project, &is_alive).map(Status::Project),
        _ => build_global(store, &is_alive).map(Status::Global),
    }
}

fn build_global(store: &Store, is_alive: &impl Fn(u32) -> bool) -> Result<GlobalStatus> {
    let mut projects = Vec::new();
    for p in project::list(store)? {
        let workspaces = workspace::list(store, &p.slug, None, false)?.len();
        let agents = agent::list_in_project(store, &p.slug)?;
        let running = count_running(store, &p.slug, &agents, is_alive);
        projects.push(ProjectStat {
            slug: p.slug,
            workspaces,
            agents: agents.len(),
            running,
        });
    }
    Ok(GlobalStatus { projects })
}

fn build_project(
    store: &Store,
    project: &str,
    is_alive: &impl Fn(u32) -> bool,
) -> Result<ProjectStatus> {
    let mut dirs = Vec::new();
    let mut trash = 0;
    for d in dir::list(store, project)? {
        if d.slug == "trash" {
            trash = workspace::list(store, project, Some("trash"), false)?.len();
            continue;
        }
        let workspaces = workspace::list(store, project, Some(&d.slug), false)?
            .into_iter()
            .map(|w| WsRow {
                slug: w.slug,
                description: w.description,
            })
            .collect();
        dirs.push(DirGroup {
            dir: d.slug,
            workspaces,
        });
    }
    let agents = agent::list_in_project(store, project)?;
    let agents_running = count_running(store, project, &agents, is_alive);
    Ok(ProjectStatus {
        project: project.to_string(),
        dirs,
        trash,
        agents_total: agents.len(),
        agents_running,
    })
}

fn build_workspace(
    store: &Store,
    project: &str,
    dir: &str,
    slug: &str,
    is_alive: &impl Fn(u32) -> bool,
) -> Result<WorkspaceStatus> {
    let description = workspace::list(store, project, Some(dir), false)?
        .into_iter()
        .find(|w| w.slug == slug)
        .and_then(|w| w.description);

    let sources = mount::list(store, project, dir, slug)?
        .into_iter()
        .map(|m| match m.kind {
            mount::MountKind::Worktree { branch } => SourceRow {
                slug: m.slug,
                kind: "worktree",
                detail: branch,
            },
            mount::MountKind::Link { target } => SourceRow {
                slug: m.slug,
                kind: "link",
                detail: target.display().to_string(),
            },
        })
        .collect();

    let agents = agent::list(store, project, dir, slug)?
        .into_iter()
        .map(|a| AgentRow {
            running: agent::running(store, project, dir, slug, &a.slug, is_alive),
            slug: a.slug,
            kind: a.kind.as_str(),
        })
        .collect();

    let path = store.workspace_path(project, dir, slug);
    let mut artifacts = Vec::new();
    if let Ok(entries) = std::fs::read_dir(path.join("docs")) {
        for entry in entries.flatten() {
            if let Some(name) = entry.file_name().to_str() {
                artifacts.push(name.to_string());
            }
        }
    }
    artifacts.sort();

    Ok(WorkspaceStatus {
        project: project.to_string(),
        dir: dir.to_string(),
        slug: slug.to_string(),
        description,
        sources,
        agents,
        artifacts,
        path,
    })
}

fn count_running(
    store: &Store,
    project: &str,
    agents: &[(String, String, agent::Agent)],
    is_alive: &impl Fn(u32) -> bool,
) -> usize {
    agents
        .iter()
        .filter(|(dir, ws, a)| agent::running(store, project, dir, ws, &a.slug, is_alive).is_some())
        .count()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::AgentKind;

    fn store() -> (tempfile::TempDir, Store) {
        let tmp = tempfile::tempdir().unwrap();
        let store = Store::new(tmp.path());
        store.ensure().unwrap();
        (tmp, store)
    }

    #[test]
    fn global_lists_projects_with_counts() {
        let (_t, s) = store();
        project::add(&s, "alpha").unwrap();
        workspace::add(&s, "alpha", "main", "ws1", None).unwrap();
        agent::resolve(&s, "alpha", "main", "ws1", AgentKind::Claude, "main").unwrap();
        project::add(&s, "beta").unwrap();

        match build(&s, &Context::default(), |_| false).unwrap() {
            Status::Global(g) => {
                let alpha = g.projects.iter().find(|p| p.slug == "alpha").unwrap();
                assert_eq!(alpha.workspaces, 1);
                assert_eq!(alpha.agents, 1);
                assert_eq!(alpha.running, 0);
                assert!(g.projects.iter().any(|p| p.slug == "beta"));
            }
            _ => panic!("expected global"),
        }
    }

    #[test]
    fn project_groups_dirs_and_counts_trash() {
        let (_t, s) = store();
        project::add(&s, "alpha").unwrap();
        workspace::add(&s, "alpha", "main", "ws1", Some("desc".into())).unwrap();
        workspace::add(&s, "alpha", "main", "ws2", None).unwrap();
        workspace::remove(&s, "alpha", "main", "ws2", false).unwrap(); // -> trash

        let ctx = Context {
            project: Some("alpha".into()),
            dir: None,
            workspace: None,
        };
        match build(&s, &ctx, |_| false).unwrap() {
            Status::Project(p) => {
                assert_eq!(p.trash, 1);
                let main = p.dirs.iter().find(|d| d.dir == "main").unwrap();
                assert!(main.workspaces.iter().any(|w| w.slug == "ws1"));
                assert!(!main.workspaces.iter().any(|w| w.slug == "ws2"));
                assert!(!p.dirs.iter().any(|d| d.dir == "trash"));
            }
            _ => panic!("expected project"),
        }
    }

    #[test]
    fn workspace_collects_sources_agents_artifacts() {
        let (_t, s) = store();
        project::add(&s, "alpha").unwrap();
        workspace::add(&s, "alpha", "main", "ws1", Some("fix".into())).unwrap();
        agent::resolve(&s, "alpha", "main", "ws1", AgentKind::Codex, "cdx").unwrap();
        let docs = s.workspace_path("alpha", "main", "ws1").join("docs");
        std::fs::write(docs.join("design.md"), "x").unwrap();

        let ctx = Context {
            project: Some("alpha".into()),
            dir: Some("main".into()),
            workspace: Some("ws1".into()),
        };
        match build(&s, &ctx, |_| false).unwrap() {
            Status::Workspace(w) => {
                assert_eq!(w.description.as_deref(), Some("fix"));
                assert!(w
                    .agents
                    .iter()
                    .any(|a| a.slug == "cdx" && a.kind == "codex"));
                assert!(w.artifacts.contains(&"design.md".to_string()));
            }
            _ => panic!("expected workspace"),
        }
    }
}
