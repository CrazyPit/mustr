use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::slug::slugify;
use crate::store::{Store, atomic_write};

/// A project-level pointer to an external directory (which may or may not be a
/// git repo). How it is brought into a workspace — symlink or worktree — is
/// chosen per workspace at materialization time, not stored here.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Source {
    /// Identity; the table key in `sources.toml`, not stored in the entry.
    #[serde(skip)]
    pub slug: String,
    /// Absolute, canonicalized path to the directory.
    pub path: PathBuf,
}

/// On-disk shape of `sources.toml`: a table keyed by slug. Unknown fields on an
/// entry (e.g. a legacy `kind` / `base_branch`) are ignored, so older registries
/// keep loading.
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

/// Registers a directory as a source. The path must exist and be a directory;
/// whether it is a git repo is decided later, at worktree time.
pub fn add(store: &Store, project: &str, path: &str, slug: Option<&str>) -> Result<Source> {
    ensure_project(store, project)?;
    let abs = canonicalize(path)?;
    if !abs.is_dir() {
        return Err(Error::InvalidSource {
            path: abs,
            reason: "not a directory".to_string(),
        });
    }
    let slug = resolve_slug(slug, &abs)?;
    insert(store, project, Source { slug, path: abs })
}

/// Removes a source entry. The real dir is untouched.
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

/// Renames a source's slug, preserving its path.
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

    #[test]
    fn add_registers_with_default_slug() {
        let (tmp, store) = project_store();
        let d = make_dir(tmp.path(), "Some Designs");

        let s = add(&store, "proj", d.to_str().unwrap(), None).unwrap();

        assert_eq!(s.slug, "some-designs");
        assert_eq!(s.path, d.canonicalize().unwrap());
        assert_eq!(list(&store, "proj").unwrap().len(), 1);
    }

    #[test]
    fn add_explicit_slug() {
        let (tmp, store) = project_store();
        let d = make_dir(tmp.path(), "stuff");
        let s = add(&store, "proj", d.to_str().unwrap(), Some("designs")).unwrap();
        assert_eq!(s.slug, "designs");
    }

    #[test]
    fn add_works_for_a_git_repo_too() {
        // A source is just a path; being a git repo is irrelevant at registration.
        let (tmp, store) = project_store();
        let repo = make_dir(tmp.path(), "backend");
        std::process::Command::new("git")
            .current_dir(&repo)
            .args(["init", "-q"])
            .status()
            .unwrap();
        let s = add(&store, "proj", repo.to_str().unwrap(), None).unwrap();
        assert_eq!(s.slug, "backend");
    }

    #[test]
    fn add_missing_path_errors() {
        let (tmp, store) = project_store();
        let missing = tmp.path().join("nope");
        assert!(matches!(
            add(&store, "proj", missing.to_str().unwrap(), None),
            Err(Error::InvalidSource { .. })
        ));
    }

    #[test]
    fn add_on_a_file_errors() {
        let (tmp, store) = project_store();
        let file = tmp.path().join("a-file");
        std::fs::write(&file, "x").unwrap();
        assert!(matches!(
            add(&store, "proj", file.to_str().unwrap(), None),
            Err(Error::InvalidSource { .. })
        ));
    }

    #[test]
    fn add_duplicate_slug_errors() {
        let (tmp, store) = project_store();
        let d = make_dir(tmp.path(), "d");
        add(&store, "proj", d.to_str().unwrap(), Some("x")).unwrap();
        assert!(matches!(
            add(&store, "proj", d.to_str().unwrap(), Some("x")),
            Err(Error::AlreadyExists { kind: "source", .. })
        ));
    }

    #[test]
    fn add_unknown_project_errors() {
        let (tmp, store) = project_store();
        let d = make_dir(tmp.path(), "d");
        assert!(matches!(
            add(&store, "ghost", d.to_str().unwrap(), None),
            Err(Error::NotFound {
                kind: "project",
                ..
            })
        ));
    }

    #[test]
    fn load_tolerates_legacy_kind_and_base_branch_fields() {
        let (tmp, store) = project_store();
        let d = make_dir(tmp.path(), "backend");
        let entry = format!(
            "[sources.backend]\nkind = \"git\"\npath = {:?}\nbase_branch = \"main\"\n",
            d.canonicalize().unwrap()
        );
        std::fs::write(store.sources_path("proj"), entry).unwrap();

        let s = get(&store, "proj", "backend").unwrap();
        assert_eq!(s.slug, "backend");
        assert_eq!(s.path, d.canonicalize().unwrap());
    }

    #[test]
    fn remove_drops_entry_but_keeps_dir() {
        let (tmp, store) = project_store();
        let d = make_dir(tmp.path(), "d");
        add(&store, "proj", d.to_str().unwrap(), Some("x")).unwrap();

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
    fn rename_moves_entry_preserving_path() {
        let (tmp, store) = project_store();
        let d = make_dir(tmp.path(), "d");
        let original = add(&store, "proj", d.to_str().unwrap(), Some("old")).unwrap();

        let renamed = rename(&store, "proj", "old", "new").unwrap();

        assert_eq!(renamed.slug, "new");
        assert_eq!(renamed.path, original.path);
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
        add(&store, "proj", a.to_str().unwrap(), Some("a")).unwrap();
        add(&store, "proj", b.to_str().unwrap(), Some("b")).unwrap();
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
        add(&store, "proj", z.to_str().unwrap(), Some("zeta")).unwrap();
        add(&store, "proj", a.to_str().unwrap(), Some("alpha")).unwrap();
        let slugs: Vec<_> = list(&store, "proj")
            .unwrap()
            .into_iter()
            .map(|s| s.slug)
            .collect();
        assert_eq!(slugs, vec!["alpha", "zeta"]);
    }
}
