use std::path::{Path, PathBuf};

use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use crate::error::{Error, Result};

/// Root of the on-disk data the tool manages, normally `~/.mustr`.
///
/// The root is injected rather than hardcoded so tests can point at a temp
/// directory. The binary builds one from `MUSTR_ROOT` or the home directory.
#[derive(Debug, Clone)]
pub struct Store {
    root: PathBuf,
}

impl Store {
    /// Creates a store rooted at `root`. Does not touch the filesystem.
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Store { root: root.into() }
    }

    /// The data root (e.g. `~/.mustr`).
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Directory holding one folder per project.
    pub fn projects_dir(&self) -> PathBuf {
        self.root.join("projects")
    }

    /// Path to the global config file.
    pub fn config_path(&self) -> PathBuf {
        self.root.join("config.toml")
    }

    /// Directory for a single project's data.
    pub fn project_dir(&self, slug: &str) -> PathBuf {
        self.projects_dir().join(slug)
    }

    /// Path to a single project's manifest.
    pub fn project_manifest_path(&self, slug: &str) -> PathBuf {
        self.project_dir(slug).join("project.toml")
    }

    /// Path to a project's sources registry.
    pub fn sources_path(&self, project: &str) -> PathBuf {
        self.project_dir(project).join("sources.toml")
    }

    /// Path to a project's config file.
    pub fn project_config_path(&self, project: &str) -> PathBuf {
        self.project_dir(project).join("config.toml")
    }

    /// Directory for a dir inside a project.
    pub fn dir_path(&self, project: &str, dir: &str) -> PathBuf {
        self.project_dir(project).join(dir)
    }

    /// Path to a dir's manifest.
    pub fn dir_manifest_path(&self, project: &str, dir: &str) -> PathBuf {
        self.dir_path(project, dir).join("dir.toml")
    }

    /// Directory for a workspace inside a project's dir.
    pub fn workspace_path(&self, project: &str, dir: &str, slug: &str) -> PathBuf {
        self.dir_path(project, dir).join(slug)
    }

    /// Path to a workspace's manifest.
    pub fn workspace_manifest_path(&self, project: &str, dir: &str, slug: &str) -> PathBuf {
        self.workspace_path(project, dir, slug)
            .join("workspace.toml")
    }

    /// The `src/` dir of a workspace, where sources are materialized.
    pub fn workspace_src_dir(&self, project: &str, dir: &str, slug: &str) -> PathBuf {
        self.workspace_path(project, dir, slug).join("src")
    }

    /// Path to an agent record inside a workspace's `agents/` dir.
    pub fn agent_manifest_path(&self, project: &str, dir: &str, ws: &str, slug: &str) -> PathBuf {
        self.workspace_path(project, dir, ws)
            .join("agents")
            .join(format!("{slug}.toml"))
    }

    /// Path to an agent's run lock (holds the live child pid while open).
    pub fn agent_lock_path(&self, project: &str, dir: &str, ws: &str, slug: &str) -> PathBuf {
        self.workspace_path(project, dir, ws)
            .join("agents")
            .join(format!("{slug}.lock"))
    }

    /// Creates the root and `projects/` directory if missing. Idempotent.
    pub fn ensure(&self) -> Result<()> {
        let projects = self.projects_dir();
        std::fs::create_dir_all(&projects).map_err(|e| Error::io(&projects, e))?;
        Ok(())
    }
}

/// Writes `contents` to `path` atomically: a temp file in the same directory is
/// written, flushed, and renamed over the target so readers never see a partial
/// file. Creates the parent directory if missing.
pub(crate) fn atomic_write(path: &Path, contents: &str) -> Result<()> {
    let parent = path
        .parent()
        .expect("manifest paths always have a parent directory");
    std::fs::create_dir_all(parent).map_err(|e| Error::io(parent, e))?;

    let tmp = path.with_extension("toml.tmp");
    std::fs::write(&tmp, contents).map_err(|e| Error::io(&tmp, e))?;
    std::fs::rename(&tmp, path).map_err(|e| Error::io(path, e))?;
    Ok(())
}

/// Current UTC time as an RFC3339 string, for `created_at` fields.
pub(crate) fn now_rfc3339() -> String {
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .expect("RFC3339 formatting of the current time is infallible")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_helpers_compose_from_root() {
        let store = Store::new("/tmp/mustr-root");
        assert_eq!(store.root(), Path::new("/tmp/mustr-root"));
        assert_eq!(store.projects_dir(), Path::new("/tmp/mustr-root/projects"));
        assert_eq!(
            store.config_path(),
            Path::new("/tmp/mustr-root/config.toml")
        );
        assert_eq!(
            store.project_dir("fix-login"),
            Path::new("/tmp/mustr-root/projects/fix-login")
        );
        assert_eq!(
            store.project_manifest_path("fix-login"),
            Path::new("/tmp/mustr-root/projects/fix-login/project.toml")
        );
    }

    #[test]
    fn ensure_creates_root_and_projects_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("nested").join(".mustr");
        let store = Store::new(&root);

        store.ensure().unwrap();

        assert!(root.is_dir());
        assert!(store.projects_dir().is_dir());
    }

    #[test]
    fn ensure_is_idempotent() {
        let tmp = tempfile::tempdir().unwrap();
        let store = Store::new(tmp.path().join(".mustr"));

        store.ensure().unwrap();
        store.ensure().unwrap();

        assert!(store.projects_dir().is_dir());
    }
}
