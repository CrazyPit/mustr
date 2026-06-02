use serde::{Deserialize, Serialize};
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

use crate::config::Config;
use crate::error::{Error, Result};
use crate::slug::slugify;
use crate::store::{atomic_write, Store};

/// A project: a named container folder under `~/.mustr/projects/<slug>/`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Project {
    /// Stable identifier (uuid v7, time-sortable). Survives renames.
    pub id: String,
    /// Human-facing name.
    pub name: String,
    /// Filesystem slug derived from the name; also the folder name.
    pub slug: String,
    /// Creation time, RFC3339. Survives renames.
    pub created_at: String,
}

/// Creates a project from `name`. The first project created also becomes the
/// default. Errors if the name has no slug or the slug is already taken.
pub fn add(store: &Store, name: &str) -> Result<Project> {
    let slug = slugify(name);
    if slug.is_empty() {
        return Err(Error::InvalidName {
            name: name.to_string(),
        });
    }
    let dir = store.project_dir(&slug);
    if dir.exists() {
        return Err(Error::AlreadyExists { slug });
    }

    let project = Project {
        id: uuid::Uuid::now_v7().to_string(),
        name: name.to_string(),
        slug: slug.clone(),
        created_at: now_rfc3339(),
    };
    std::fs::create_dir_all(&dir).map_err(|e| Error::io(&dir, e))?;
    write_manifest(store, &project)?;

    let mut config = Config::load(store)?;
    if config.default_project.is_none() {
        config.default_project = Some(slug);
        config.save(store)?;
    }
    Ok(project)
}

/// Lists projects sorted by name (case-insensitive). Folders without a
/// `project.toml` are ignored.
pub fn list(store: &Store) -> Result<Vec<Project>> {
    let dir = store.projects_dir();
    let entries = match std::fs::read_dir(&dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(Error::io(&dir, e)),
    };

    let mut projects = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|e| Error::io(&dir, e))?;
        let path = entry.path();
        if !path.join("project.toml").is_file() {
            continue;
        }
        let raw = std::fs::read_to_string(path.join("project.toml"))
            .map_err(|e| Error::io(path.join("project.toml"), e))?;
        let project: Project = toml::from_str(&raw).map_err(|source| Error::TomlRead {
            path: path.join("project.toml"),
            source,
        })?;
        projects.push(project);
    }
    projects.sort_by_key(|p| p.name.to_lowercase());
    Ok(projects)
}

/// Removes a project by slug. If it was the default, the default moves to the
/// first remaining project (alphabetical by slug), or clears when none remain.
pub fn remove(store: &Store, slug: &str) -> Result<()> {
    let dir = store.project_dir(slug);
    if !dir.exists() {
        return Err(Error::NotFound {
            slug: slug.to_string(),
        });
    }
    std::fs::remove_dir_all(&dir).map_err(|e| Error::io(&dir, e))?;

    let mut config = Config::load(store)?;
    if config.default_project.as_deref() == Some(slug) {
        let mut remaining: Vec<String> = list(store)?.into_iter().map(|p| p.slug).collect();
        remaining.sort();
        config.default_project = remaining.into_iter().next();
        config.save(store)?;
    }
    Ok(())
}

/// Renames a project, re-slugging from the new name. Preserves `id` and
/// `created_at`; moves the default with it when applicable.
pub fn rename(store: &Store, slug: &str, new_name: &str) -> Result<Project> {
    let mut project = read_manifest(store, slug)?;

    let new_slug = slugify(new_name);
    if new_slug.is_empty() {
        return Err(Error::InvalidName {
            name: new_name.to_string(),
        });
    }

    if new_slug == project.slug {
        project.name = new_name.to_string();
        write_manifest(store, &project)?;
        return Ok(project);
    }

    let new_dir = store.project_dir(&new_slug);
    if new_dir.exists() {
        return Err(Error::AlreadyExists { slug: new_slug });
    }
    std::fs::rename(store.project_dir(slug), &new_dir).map_err(|e| Error::io(&new_dir, e))?;

    project.name = new_name.to_string();
    project.slug = new_slug.clone();
    write_manifest(store, &project)?;

    let mut config = Config::load(store)?;
    if config.default_project.as_deref() == Some(slug) {
        config.default_project = Some(new_slug);
        config.save(store)?;
    }
    Ok(project)
}

fn read_manifest(store: &Store, slug: &str) -> Result<Project> {
    let path = store.project_manifest_path(slug);
    let raw = match std::fs::read_to_string(&path) {
        Ok(raw) => raw,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err(Error::NotFound {
                slug: slug.to_string(),
            })
        }
        Err(e) => return Err(Error::io(&path, e)),
    };
    toml::from_str(&raw).map_err(|source| Error::TomlRead { path, source })
}

fn write_manifest(store: &Store, project: &Project) -> Result<()> {
    let path = store.project_manifest_path(&project.slug);
    let raw = toml::to_string_pretty(project).map_err(|source| Error::TomlWrite {
        path: path.clone(),
        source,
    })?;
    atomic_write(&path, &raw)
}

fn now_rfc3339() -> String {
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .expect("RFC3339 formatting of the current time is infallible")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> (tempfile::TempDir, Store) {
        let tmp = tempfile::tempdir().unwrap();
        let store = Store::new(tmp.path());
        store.ensure().unwrap();
        (tmp, store)
    }

    #[test]
    fn add_creates_dir_and_manifest() {
        let (_tmp, store) = store();

        let project = add(&store, "Fix Login").unwrap();

        assert_eq!(project.name, "Fix Login");
        assert_eq!(project.slug, "fix-login");
        assert!(!project.id.is_empty());
        assert!(OffsetDateTime::parse(&project.created_at, &Rfc3339).is_ok());

        let manifest = store.project_manifest_path("fix-login");
        assert!(manifest.is_file());
        let on_disk: Project = toml::from_str(&std::fs::read_to_string(manifest).unwrap()).unwrap();
        assert_eq!(on_disk, project);
    }

    #[test]
    fn add_duplicate_slug_errors() {
        let (_tmp, store) = store();
        add(&store, "Fix Login").unwrap();

        // Same name, and a different name that slugifies the same, both collide.
        assert!(matches!(
            add(&store, "Fix Login"),
            Err(Error::AlreadyExists { .. })
        ));
        assert!(matches!(
            add(&store, "fix   login"),
            Err(Error::AlreadyExists { .. })
        ));
    }

    #[test]
    fn add_empty_slug_is_invalid_name() {
        let (_tmp, store) = store();
        assert!(matches!(
            add(&store, "!!! ???"),
            Err(Error::InvalidName { .. })
        ));
    }

    #[test]
    fn first_add_sets_default_and_later_adds_do_not_change_it() {
        let (_tmp, store) = store();

        add(&store, "Alpha").unwrap();
        assert_eq!(
            Config::load(&store).unwrap().default_project.as_deref(),
            Some("alpha")
        );

        add(&store, "Beta").unwrap();
        assert_eq!(
            Config::load(&store).unwrap().default_project.as_deref(),
            Some("alpha")
        );
    }

    #[test]
    fn list_is_empty_on_fresh_store() {
        let (_tmp, store) = store();
        assert!(list(&store).unwrap().is_empty());
    }

    #[test]
    fn list_returns_projects_sorted_by_name() {
        let (_tmp, store) = store();
        add(&store, "Zebra").unwrap();
        add(&store, "alpha").unwrap();
        add(&store, "Mango").unwrap();

        let names: Vec<_> = list(&store).unwrap().into_iter().map(|p| p.name).collect();
        assert_eq!(names, vec!["alpha", "Mango", "Zebra"]);
    }

    #[test]
    fn list_ignores_entries_without_manifest() {
        let (_tmp, store) = store();
        add(&store, "Real").unwrap();
        std::fs::create_dir(store.projects_dir().join("stray-dir")).unwrap();
        std::fs::write(store.projects_dir().join("stray-file"), "x").unwrap();

        let slugs: Vec<_> = list(&store).unwrap().into_iter().map(|p| p.slug).collect();
        assert_eq!(slugs, vec!["real"]);
    }

    #[test]
    fn remove_deletes_the_folder() {
        let (_tmp, store) = store();
        add(&store, "Doomed").unwrap();

        remove(&store, "doomed").unwrap();

        assert!(!store.project_dir("doomed").exists());
        assert!(list(&store).unwrap().is_empty());
    }

    #[test]
    fn remove_unknown_slug_errors() {
        let (_tmp, store) = store();
        assert!(matches!(
            remove(&store, "ghost"),
            Err(Error::NotFound { .. })
        ));
    }

    #[test]
    fn remove_default_reassigns_then_clears() {
        let (_tmp, store) = store();
        add(&store, "Alpha").unwrap(); // default = alpha
        add(&store, "Beta").unwrap();

        remove(&store, "alpha").unwrap();
        assert_eq!(
            Config::load(&store).unwrap().default_project.as_deref(),
            Some("beta")
        );

        remove(&store, "beta").unwrap();
        assert_eq!(Config::load(&store).unwrap().default_project, None);
    }

    #[test]
    fn rename_moves_folder_and_preserves_identity() {
        let (_tmp, store) = store();
        let original = add(&store, "Fix Login").unwrap();

        let renamed = rename(&store, "fix-login", "Login Fixes").unwrap();

        assert_eq!(renamed.name, "Login Fixes");
        assert_eq!(renamed.slug, "login-fixes");
        assert_eq!(renamed.id, original.id);
        assert_eq!(renamed.created_at, original.created_at);
        assert!(!store.project_dir("fix-login").exists());
        assert!(store.project_manifest_path("login-fixes").is_file());
    }

    #[test]
    fn rename_to_same_slug_updates_name_only() {
        let (_tmp, store) = store();
        add(&store, "Fix Login").unwrap();

        let renamed = rename(&store, "fix-login", "Fix  Login!").unwrap();

        assert_eq!(renamed.name, "Fix  Login!");
        assert_eq!(renamed.slug, "fix-login");
        assert!(store.project_dir("fix-login").is_dir());
    }

    #[test]
    fn rename_to_taken_slug_errors() {
        let (_tmp, store) = store();
        add(&store, "Alpha").unwrap();
        add(&store, "Beta").unwrap();

        assert!(matches!(
            rename(&store, "alpha", "Beta"),
            Err(Error::AlreadyExists { .. })
        ));
    }

    #[test]
    fn rename_unknown_slug_errors() {
        let (_tmp, store) = store();
        assert!(matches!(
            rename(&store, "ghost", "Whatever"),
            Err(Error::NotFound { .. })
        ));
    }

    #[test]
    fn rename_to_empty_slug_is_invalid_name() {
        let (_tmp, store) = store();
        add(&store, "Alpha").unwrap();
        assert!(matches!(
            rename(&store, "alpha", "!!!"),
            Err(Error::InvalidName { .. })
        ));
    }

    #[test]
    fn rename_moves_the_default_with_it() {
        let (_tmp, store) = store();
        add(&store, "Alpha").unwrap(); // default = alpha

        rename(&store, "alpha", "Gamma").unwrap();

        assert_eq!(
            Config::load(&store).unwrap().default_project.as_deref(),
            Some("gamma")
        );
    }
}
