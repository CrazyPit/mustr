use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::slug::slugify;
use crate::store::{Store, atomic_write, now_rfc3339};

/// A project: a container folder under `~/.mustr/projects/<slug>/`.
///
/// The slug is the folder name and the sole identity — it is not stored in the
/// manifest but derived from the directory, so renaming a project is literally
/// renaming its folder.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Project {
    /// Stable identifier (uuid v7, time-sortable). Survives renames.
    pub id: String,
    /// Folder name; the project's identity. Derived from the directory, not the
    /// manifest.
    #[serde(skip)]
    pub slug: String,
    /// Creation time, RFC3339. Survives renames.
    pub created_at: String,
}

/// Creates a project. `input` is slugified into the folder name. Errors if
/// `input` has no slug or the slug is already taken.
pub fn add(store: &Store, input: &str) -> Result<Project> {
    let slug = slugify(input);
    if slug.is_empty() {
        return Err(Error::InvalidName {
            name: input.to_string(),
        });
    }
    let dir = store.project_dir(&slug);
    if dir.exists() {
        return Err(Error::AlreadyExists {
            kind: "project",
            slug,
        });
    }

    let project = Project {
        id: uuid::Uuid::now_v7().to_string(),
        slug,
        created_at: now_rfc3339(),
    };
    std::fs::create_dir_all(&dir).map_err(|e| Error::io(&dir, e))?;
    write_manifest(store, &project)?;

    // A new project ships with its reserved dirs.
    crate::dir::ensure_defaults(store, &project.slug)?;
    Ok(project)
}

/// Lists projects sorted by slug. Folders without a `project.toml` are ignored.
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
        let Some(slug) = entry.file_name().to_str().map(str::to_string) else {
            continue;
        };
        if store.project_manifest_path(&slug).is_file() {
            projects.push(read_manifest(store, &slug)?);
        }
    }
    projects.sort_by(|a, b| a.slug.cmp(&b.slug));
    Ok(projects)
}

/// Removes a project by slug.
pub fn remove(store: &Store, slug: &str) -> Result<()> {
    let dir = store.project_dir(slug);
    if !dir.exists() {
        return Err(Error::NotFound {
            kind: "project",
            slug: slug.to_string(),
        });
    }
    std::fs::remove_dir_all(&dir).map_err(|e| Error::io(&dir, e))?;
    Ok(())
}

/// Renames a project, re-slugging from the new name. Preserves `id` and
/// `created_at`.
pub fn rename(store: &Store, slug: &str, new: &str) -> Result<Project> {
    if !store.project_dir(slug).exists() {
        return Err(Error::NotFound {
            kind: "project",
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
        return read_manifest(store, slug);
    }

    let new_dir = store.project_dir(&new_slug);
    if new_dir.exists() {
        return Err(Error::AlreadyExists {
            kind: "project",
            slug: new_slug,
        });
    }
    std::fs::rename(store.project_dir(slug), &new_dir).map_err(|e| Error::io(&new_dir, e))?;
    read_manifest(store, &new_slug)
}

fn read_manifest(store: &Store, slug: &str) -> Result<Project> {
    let path = store.project_manifest_path(slug);
    let raw = match std::fs::read_to_string(&path) {
        Ok(raw) => raw,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err(Error::NotFound {
                kind: "project",
                slug: slug.to_string(),
            });
        }
        Err(e) => return Err(Error::io(&path, e)),
    };
    let mut project: Project =
        toml::from_str(&raw).map_err(|source| Error::TomlRead { path, source })?;
    // slug is not stored; it is the folder name.
    project.slug = slug.to_string();
    Ok(project)
}

fn write_manifest(store: &Store, project: &Project) -> Result<()> {
    let path = store.project_manifest_path(&project.slug);
    let raw = toml::to_string_pretty(project).map_err(|source| Error::TomlWrite {
        path: path.clone(),
        source,
    })?;
    atomic_write(&path, &raw)
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::OffsetDateTime;
    use time::format_description::well_known::Rfc3339;

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

        assert_eq!(project.slug, "fix-login");
        assert!(!project.id.is_empty());
        assert!(OffsetDateTime::parse(&project.created_at, &Rfc3339).is_ok());

        assert!(store.project_manifest_path("fix-login").is_file());
        // slug is derived from the folder, not stored in the manifest.
        assert_eq!(read_manifest(&store, "fix-login").unwrap(), project);
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
    fn list_is_empty_on_fresh_store() {
        let (_tmp, store) = store();
        assert!(list(&store).unwrap().is_empty());
    }

    #[test]
    fn list_returns_projects_sorted_by_slug() {
        let (_tmp, store) = store();
        add(&store, "Zebra").unwrap();
        add(&store, "alpha").unwrap();
        add(&store, "Mango").unwrap();

        let slugs: Vec<_> = list(&store).unwrap().into_iter().map(|p| p.slug).collect();
        assert_eq!(slugs, vec!["alpha", "mango", "zebra"]);
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
    fn rename_moves_folder_and_preserves_identity() {
        let (_tmp, store) = store();
        let original = add(&store, "Fix Login").unwrap();

        let renamed = rename(&store, "fix-login", "Login Fixes").unwrap();

        assert_eq!(renamed.slug, "login-fixes");
        assert_eq!(renamed.id, original.id);
        assert_eq!(renamed.created_at, original.created_at);
        assert!(!store.project_dir("fix-login").exists());
        assert!(store.project_manifest_path("login-fixes").is_file());
    }

    #[test]
    fn rename_to_same_slug_is_a_noop() {
        let (_tmp, store) = store();
        let original = add(&store, "Fix Login").unwrap();

        // Input slugifies back to the existing slug — nothing to move.
        let renamed = rename(&store, "fix-login", "Fix  Login!").unwrap();

        assert_eq!(renamed.slug, "fix-login");
        assert_eq!(renamed.id, original.id);
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
}
