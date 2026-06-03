use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::slug::slugify;
use crate::store::{Store, atomic_write};

/// What a source points at.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SourceKind {
    /// A git repository; worktrees are cut from it.
    Git,
    /// A plain directory; symlinked into workspaces.
    Dir,
}

/// A project-level pointer to an external git repo or directory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Source {
    /// Identity; the table key in `sources.toml`, not stored in the entry.
    #[serde(skip)]
    pub slug: String,
    /// Whether this is a git repo or a plain dir.
    pub kind: SourceKind,
    /// Absolute, canonicalized path to the repo/dir.
    pub path: PathBuf,
    /// Base branch for git sources; absent for dirs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_branch: Option<String>,
}

/// On-disk shape of `sources.toml`: a table keyed by slug.
#[derive(Default, Serialize, Deserialize)]
struct Registry {
    #[serde(default)]
    sources: BTreeMap<String, Source>,
}

/// Lists a project's sources, sorted by slug.
pub fn list(store: &Store, project: &str) -> Result<Vec<Source>> {
    ensure_project(store, project)?;
    Ok(load(store, project)?.into_values().collect())
}

/// Looks up a single source by slug.
pub fn get(store: &Store, project: &str, slug: &str) -> Result<Source> {
    ensure_project(store, project)?;
    load(store, project)?
        .remove(slug)
        .ok_or_else(|| Error::NotFound {
            kind: "source",
            slug: slug.to_string(),
        })
}

/// Registers a git repository. `path` must be a git work tree.
pub fn add_git(
    store: &Store,
    project: &str,
    path: &str,
    slug: Option<&str>,
    base_branch: Option<&str>,
) -> Result<Source> {
    ensure_project(store, project)?;
    let abs = canonicalize(path)?;
    if !is_git_worktree(&abs) {
        return Err(Error::InvalidSource {
            path: abs,
            reason: "not a git repository".to_string(),
        });
    }
    let slug = resolve_slug(slug, &abs)?;
    let branch = match base_branch {
        Some(b) => b.to_string(),
        None => detect_base_branch(&abs),
    };
    insert(
        store,
        project,
        Source {
            slug,
            kind: SourceKind::Git,
            path: abs,
            base_branch: Some(branch),
        },
    )
}

/// Registers a plain directory.
pub fn add_dir(store: &Store, project: &str, path: &str, slug: Option<&str>) -> Result<Source> {
    ensure_project(store, project)?;
    let abs = canonicalize(path)?;
    if !abs.is_dir() {
        return Err(Error::InvalidSource {
            path: abs,
            reason: "not a directory".to_string(),
        });
    }
    let slug = resolve_slug(slug, &abs)?;
    insert(
        store,
        project,
        Source {
            slug,
            kind: SourceKind::Dir,
            path: abs,
            base_branch: None,
        },
    )
}

/// Removes a source entry. The real repo/dir is untouched.
pub fn remove(store: &Store, project: &str, slug: &str) -> Result<()> {
    ensure_project(store, project)?;
    let mut sources = load(store, project)?;
    if sources.remove(slug).is_none() {
        return Err(Error::NotFound {
            kind: "source",
            slug: slug.to_string(),
        });
    }
    save(store, project, &sources)
}

/// Renames a source's slug, preserving its kind/path/base_branch.
pub fn rename(store: &Store, project: &str, slug: &str, new: &str) -> Result<Source> {
    ensure_project(store, project)?;
    let mut sources = load(store, project)?;
    let Some(mut source) = sources.remove(slug) else {
        return Err(Error::NotFound {
            kind: "source",
            slug: slug.to_string(),
        });
    };

    let new_slug = slugify(new);
    if new_slug.is_empty() {
        return Err(Error::InvalidName {
            name: new.to_string(),
        });
    }
    if new_slug != slug && sources.contains_key(&new_slug) {
        return Err(Error::AlreadyExists {
            kind: "source",
            slug: new_slug,
        });
    }

    source.slug = new_slug.clone();
    sources.insert(new_slug, source.clone());
    save(store, project, &sources)?;
    Ok(source)
}

fn ensure_project(store: &Store, project: &str) -> Result<()> {
    if store.project_manifest_path(project).is_file() {
        Ok(())
    } else {
        Err(Error::NotFound {
            kind: "project",
            slug: project.to_string(),
        })
    }
}

fn insert(store: &Store, project: &str, source: Source) -> Result<Source> {
    let mut sources = load(store, project)?;
    if sources.contains_key(&source.slug) {
        return Err(Error::AlreadyExists {
            kind: "source",
            slug: source.slug.clone(),
        });
    }
    sources.insert(source.slug.clone(), source.clone());
    save(store, project, &sources)?;
    Ok(source)
}

fn load(store: &Store, project: &str) -> Result<BTreeMap<String, Source>> {
    let path = store.sources_path(project);
    let raw = match std::fs::read_to_string(&path) {
        Ok(raw) => raw,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(BTreeMap::new()),
        Err(e) => return Err(Error::io(&path, e)),
    };
    let mut registry: Registry =
        toml::from_str(&raw).map_err(|source| Error::TomlRead { path, source })?;
    for (slug, source) in registry.sources.iter_mut() {
        source.slug = slug.clone();
    }
    Ok(registry.sources)
}

fn save(store: &Store, project: &str, sources: &BTreeMap<String, Source>) -> Result<()> {
    let path = store.sources_path(project);
    let registry = Registry {
        sources: sources.clone(),
    };
    let raw = toml::to_string_pretty(&registry).map_err(|source| Error::TomlWrite {
        path: path.clone(),
        source,
    })?;
    atomic_write(&path, &raw)
}

fn canonicalize(path: &str) -> Result<PathBuf> {
    Path::new(path)
        .canonicalize()
        .map_err(|_| Error::InvalidSource {
            path: PathBuf::from(path),
            reason: "path does not exist".to_string(),
        })
}

fn resolve_slug(slug: Option<&str>, abs: &Path) -> Result<String> {
    let raw = match slug {
        Some(s) => s.to_string(),
        None => abs
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned(),
    };
    let slug = slugify(&raw);
    if slug.is_empty() {
        return Err(Error::InvalidName { name: raw });
    }
    Ok(slug)
}

fn is_git_worktree(repo: &Path) -> bool {
    git(repo, &["rev-parse", "--is-inside-work-tree"]).as_deref() == Some("true")
}

/// Picks a base branch: `origin/HEAD`, then a common name that exists, then the
/// current branch, then `main`.
fn detect_base_branch(repo: &Path) -> String {
    if let Some(head) = git(
        repo,
        &["symbolic-ref", "--short", "refs/remotes/origin/HEAD"],
    ) {
        return head.strip_prefix("origin/").unwrap_or(&head).to_string();
    }
    for name in ["main", "master", "develop", "trunk"] {
        if git(
            repo,
            &[
                "rev-parse",
                "--verify",
                "--quiet",
                &format!("refs/heads/{name}"),
            ],
        )
        .is_some()
        {
            return name.to_string();
        }
    }
    match git(repo, &["rev-parse", "--abbrev-ref", "HEAD"]) {
        Some(branch) if branch != "HEAD" => branch,
        _ => "main".to_string(),
    }
}

/// Runs `git` in `repo`, returning trimmed stdout on success (None otherwise).
fn git(repo: &Path, args: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .current_dir(repo)
        .args(args)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let out = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!out.is_empty()).then_some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project_store() -> (tempfile::TempDir, Store) {
        let tmp = tempfile::tempdir().unwrap();
        let store = Store::new(tmp.path());
        store.ensure().unwrap();
        crate::project::add(&store, "proj").unwrap();
        (tmp, store)
    }

    fn make_dir(parent: &Path, name: &str) -> PathBuf {
        let p = parent.join(name);
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    fn run_git(dir: &Path, args: &[&str]) {
        let status = Command::new("git")
            .current_dir(dir)
            .args(args)
            .status()
            .unwrap();
        assert!(status.success(), "git {args:?} failed");
    }

    fn init_repo(parent: &Path, name: &str, branch: &str) -> PathBuf {
        let p = make_dir(parent, name);
        run_git(&p, &["init", "-q"]);
        run_git(
            &p,
            &["symbolic-ref", "HEAD", &format!("refs/heads/{branch}")],
        );
        run_git(&p, &["config", "user.email", "t@example.com"]);
        run_git(&p, &["config", "user.name", "Test"]);
        std::fs::write(p.join("README"), "x").unwrap();
        run_git(&p, &["add", "-A"]);
        run_git(&p, &["commit", "-q", "-m", "init"]);
        p
    }

    #[test]
    fn add_dir_registers_with_default_slug() {
        let (tmp, store) = project_store();
        let d = make_dir(tmp.path(), "Some Designs");

        let s = add_dir(&store, "proj", d.to_str().unwrap(), None).unwrap();

        assert_eq!(s.slug, "some-designs");
        assert_eq!(s.kind, SourceKind::Dir);
        assert_eq!(s.path, d.canonicalize().unwrap());
        assert_eq!(s.base_branch, None);
        assert_eq!(list(&store, "proj").unwrap().len(), 1);
    }

    #[test]
    fn add_dir_explicit_slug() {
        let (tmp, store) = project_store();
        let d = make_dir(tmp.path(), "stuff");
        let s = add_dir(&store, "proj", d.to_str().unwrap(), Some("designs")).unwrap();
        assert_eq!(s.slug, "designs");
    }

    #[test]
    fn add_dir_missing_path_errors() {
        let (tmp, store) = project_store();
        let missing = tmp.path().join("nope");
        assert!(matches!(
            add_dir(&store, "proj", missing.to_str().unwrap(), None),
            Err(Error::InvalidSource { .. })
        ));
    }

    #[test]
    fn add_dir_on_a_file_errors() {
        let (tmp, store) = project_store();
        let file = tmp.path().join("a-file");
        std::fs::write(&file, "x").unwrap();
        assert!(matches!(
            add_dir(&store, "proj", file.to_str().unwrap(), None),
            Err(Error::InvalidSource { .. })
        ));
    }

    #[test]
    fn add_duplicate_slug_errors() {
        let (tmp, store) = project_store();
        let d = make_dir(tmp.path(), "d");
        add_dir(&store, "proj", d.to_str().unwrap(), Some("x")).unwrap();
        assert!(matches!(
            add_dir(&store, "proj", d.to_str().unwrap(), Some("x")),
            Err(Error::AlreadyExists { kind: "source", .. })
        ));
    }

    #[test]
    fn add_dir_unknown_project_errors() {
        let (tmp, store) = project_store();
        let d = make_dir(tmp.path(), "d");
        assert!(matches!(
            add_dir(&store, "ghost", d.to_str().unwrap(), None),
            Err(Error::NotFound {
                kind: "project",
                ..
            })
        ));
    }

    #[test]
    fn add_git_detects_main_branch() {
        let (tmp, store) = project_store();
        let repo = init_repo(tmp.path(), "backend", "main");

        let s = add_git(&store, "proj", repo.to_str().unwrap(), None, None).unwrap();

        assert_eq!(s.slug, "backend");
        assert_eq!(s.kind, SourceKind::Git);
        assert_eq!(s.base_branch.as_deref(), Some("main"));
        assert_eq!(s.path, repo.canonicalize().unwrap());
    }

    #[test]
    fn add_git_detects_master_branch() {
        let (tmp, store) = project_store();
        let repo = init_repo(tmp.path(), "legacy", "master");
        let s = add_git(&store, "proj", repo.to_str().unwrap(), None, None).unwrap();
        assert_eq!(s.base_branch.as_deref(), Some("master"));
    }

    #[test]
    fn add_git_uses_current_branch_when_uncommon() {
        let (tmp, store) = project_store();
        let repo = init_repo(tmp.path(), "weird", "feature-x");
        let s = add_git(&store, "proj", repo.to_str().unwrap(), None, None).unwrap();
        assert_eq!(s.base_branch.as_deref(), Some("feature-x"));
    }

    #[test]
    fn add_git_base_branch_override() {
        let (tmp, store) = project_store();
        let repo = init_repo(tmp.path(), "backend", "main");
        let s = add_git(
            &store,
            "proj",
            repo.to_str().unwrap(),
            None,
            Some("release"),
        )
        .unwrap();
        assert_eq!(s.base_branch.as_deref(), Some("release"));
    }

    #[test]
    fn add_git_on_non_repo_errors() {
        let (tmp, store) = project_store();
        let d = make_dir(tmp.path(), "plain");
        assert!(matches!(
            add_git(&store, "proj", d.to_str().unwrap(), None, None),
            Err(Error::InvalidSource { .. })
        ));
    }

    #[test]
    fn remove_drops_entry_but_keeps_dir() {
        let (tmp, store) = project_store();
        let d = make_dir(tmp.path(), "d");
        add_dir(&store, "proj", d.to_str().unwrap(), Some("x")).unwrap();

        remove(&store, "proj", "x").unwrap();

        assert!(list(&store, "proj").unwrap().is_empty());
        assert!(d.is_dir());
    }

    #[test]
    fn remove_unknown_errors() {
        let (_tmp, store) = project_store();
        assert!(matches!(
            remove(&store, "proj", "ghost"),
            Err(Error::NotFound { kind: "source", .. })
        ));
    }

    #[test]
    fn rename_moves_entry_preserving_fields() {
        let (tmp, store) = project_store();
        let d = make_dir(tmp.path(), "d");
        let original = add_dir(&store, "proj", d.to_str().unwrap(), Some("old")).unwrap();

        let renamed = rename(&store, "proj", "old", "new").unwrap();

        assert_eq!(renamed.slug, "new");
        assert_eq!(renamed.path, original.path);
        assert_eq!(renamed.kind, original.kind);
        let slugs: Vec<_> = list(&store, "proj")
            .unwrap()
            .into_iter()
            .map(|s| s.slug)
            .collect();
        assert_eq!(slugs, vec!["new"]);
    }

    #[test]
    fn rename_unknown_errors() {
        let (_tmp, store) = project_store();
        assert!(matches!(
            rename(&store, "proj", "ghost", "x"),
            Err(Error::NotFound { kind: "source", .. })
        ));
    }

    #[test]
    fn rename_collision_errors() {
        let (tmp, store) = project_store();
        let a = make_dir(tmp.path(), "a");
        let b = make_dir(tmp.path(), "b");
        add_dir(&store, "proj", a.to_str().unwrap(), Some("a")).unwrap();
        add_dir(&store, "proj", b.to_str().unwrap(), Some("b")).unwrap();
        assert!(matches!(
            rename(&store, "proj", "a", "b"),
            Err(Error::AlreadyExists { kind: "source", .. })
        ));
    }

    #[test]
    fn list_is_empty_without_file_and_sorted_otherwise() {
        let (tmp, store) = project_store();
        assert!(list(&store, "proj").unwrap().is_empty());

        let z = make_dir(tmp.path(), "z");
        let a = make_dir(tmp.path(), "a");
        add_dir(&store, "proj", z.to_str().unwrap(), Some("zeta")).unwrap();
        add_dir(&store, "proj", a.to_str().unwrap(), Some("alpha")).unwrap();
        let slugs: Vec<_> = list(&store, "proj")
            .unwrap()
            .into_iter()
            .map(|s| s.slug)
            .collect();
        assert_eq!(slugs, vec!["alpha", "zeta"]);
    }
}
