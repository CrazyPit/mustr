use std::path::Path;

use crate::store::Store;

/// Where the current working directory sits inside the data root: which project,
/// dir, and workspace (each present only if the path reaches that level and the
/// corresponding entity actually exists).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Context {
    pub project: Option<String>,
    pub dir: Option<String>,
    pub workspace: Option<String>,
}

/// Derives the [`Context`] from `cwd` relative to the store's `projects/` dir.
///
/// Pure over `(store, cwd)` — it reads the filesystem but never the process cwd,
/// so it is testable with arbitrary paths. Components below the workspace (e.g.
/// inside `src/<repo>/`) are ignored.
pub fn context_from(store: &Store, cwd: &Path) -> Context {
    let empty = Context::default();
    let (Ok(projects), Ok(cwd)) = (store.projects_dir().canonicalize(), cwd.canonicalize()) else {
        return empty;
    };
    let Ok(rest) = cwd.strip_prefix(&projects) else {
        return empty;
    };
    let comps: Vec<String> = rest
        .components()
        .filter_map(|c| match c {
            std::path::Component::Normal(s) => s.to_str().map(str::to_string),
            _ => None,
        })
        .collect();

    let project = match comps.first() {
        Some(p) if store.project_manifest_path(p).is_file() => p.clone(),
        _ => return empty,
    };
    let dir = comps
        .get(1)
        .filter(|d| store.dir_manifest_path(&project, d).is_file())
        .cloned();
    let workspace = match (&dir, comps.get(2)) {
        (Some(dir), Some(ws)) if store.workspace_manifest_path(&project, dir, ws).is_file() => {
            Some(ws.clone())
        }
        _ => None,
    };

    Context {
        project: Some(project),
        dir,
        workspace,
    }
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
    fn full_workspace_path() {
        let (_tmp, store) = setup();
        let ctx = context_from(&store, &store.workspace_path("proj", "main", "ws"));
        assert_eq!(ctx.project.as_deref(), Some("proj"));
        assert_eq!(ctx.dir.as_deref(), Some("main"));
        assert_eq!(ctx.workspace.as_deref(), Some("ws"));
    }

    #[test]
    fn project_only() {
        let (_tmp, store) = setup();
        let ctx = context_from(&store, &store.project_dir("proj"));
        assert_eq!(ctx.project.as_deref(), Some("proj"));
        assert_eq!(ctx.dir, None);
        assert_eq!(ctx.workspace, None);
    }

    #[test]
    fn project_and_dir_without_workspace() {
        let (_tmp, store) = setup();
        let ctx = context_from(&store, &store.dir_path("proj", "main"));
        assert_eq!(ctx.project.as_deref(), Some("proj"));
        assert_eq!(ctx.dir.as_deref(), Some("main"));
        assert_eq!(ctx.workspace, None);
    }

    #[test]
    fn deeper_than_workspace_is_clamped() {
        let (_tmp, store) = setup();
        let deep = store.workspace_path("proj", "main", "ws").join("src");
        let ctx = context_from(&store, &deep);
        assert_eq!(ctx.project.as_deref(), Some("proj"));
        assert_eq!(ctx.dir.as_deref(), Some("main"));
        assert_eq!(ctx.workspace.as_deref(), Some("ws"));
    }

    #[test]
    fn at_root_is_empty() {
        let (_tmp, store) = setup();
        assert_eq!(context_from(&store, store.root()), Context::default());
    }

    #[test]
    fn outside_root_is_empty() {
        let (tmp, store) = setup();
        let outside = tmp.path().parent().unwrap();
        assert_eq!(context_from(&store, outside), Context::default());
    }

    #[test]
    fn unknown_project_component_is_empty() {
        let (_tmp, store) = setup();
        let ghost = store.project_dir("ghost");
        std::fs::create_dir_all(&ghost).unwrap();
        assert_eq!(context_from(&store, &ghost), Context::default());
    }
}
