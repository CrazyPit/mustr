use serde::{Deserialize, Serialize};

use crate::config::Config;
use crate::error::{Error, Result};
use crate::slug::slugify;
use crate::store::{atomic_write, now_rfc3339, Store};

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

/// Creates a project. `input` is slugified into the folder name. The first
/// project created also becomes the default. Errors if `input` has no slug or
/// the slug is already taken.
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
    // First project created becomes the default; an existing default stays put.
    resolve_default(store)?;
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

/// Removes a project by slug. If it was the default, the default moves to the
/// first remaining project (by slug), or clears when none remain.
pub fn remove(store: &Store, slug: &str) -> Result<()> {
    let dir = store.project_dir(slug);
    if !dir.exists() {
        return Err(Error::NotFound {
            kind: "project",
            slug: slug.to_string(),
        });
    }
    std::fs::remove_dir_all(&dir).map_err(|e| Error::io(&dir, e))?;

    // Reassign the default if it pointed at the just-removed project.
    resolve_default(store)?;
    Ok(())
}

/// Renames a project, re-slugging from the new name. Preserves `id` and
/// `created_at`; moves the default with it when applicable.
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

    let mut config = Config::load(store)?;
    if config.default_project.as_deref() == Some(slug) {
        config.default_project = Some(new_slug.clone());
        config.save(store)?;
    }
    read_manifest(store, &new_slug)
}

/// Sets the default project to `slug`. Errors if no such project exists.
pub fn set_default(store: &Store, slug: &str) -> Result<()> {
    if !store.project_dir(slug).exists() {
        return Err(Error::NotFound {
            kind: "project",
            slug: slug.to_string(),
        });
    }
    let mut config = Config::load(store)?;
    config.default_project = Some(slug.to_string());
    config.save(store)
}

/// Returns the effective default project, healing a dangling or empty default.
///
/// If `default_project` names an existing project it is returned unchanged.
/// Otherwise — the configured default was deleted (by `mustr` or by hand), or
/// none was set while projects exist — the first project by slug is chosen and
/// persisted. With no projects the default is cleared. Persists only on change.
pub fn resolve_default(store: &Store) -> Result<Option<String>> {
    let mut config = Config::load(store)?;
    let projects = list(store)?;

    let effective = match &config.default_project {
        Some(slug) if projects.iter().any(|p| &p.slug == slug) => Some(slug.clone()),
        _ => projects.first().map(|p| p.slug.clone()),
    };

    if config.default_project != effective {
        config.default_project = effective.clone();
        config.save(store)?;
    }
    Ok(effective)
}

fn read_manifest(store: &Store, slug: &str) -> Result<Project> {
    let path = store.project_manifest_path(slug);
    let raw = match std::fs::read_to_string(&path) {
        Ok(raw) => raw,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err(Error::NotFound {
                kind: "project",
                slug: slug.to_string(),
            })
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
    use time::format_description::well_known::Rfc3339;
    use time::OffsetDateTime;

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

    #[test]
    fn set_default_persists_the_choice() {
        let (_tmp, store) = store();
        add(&store, "Alpha").unwrap(); // default = alpha
        add(&store, "Beta").unwrap();

        set_default(&store, "beta").unwrap();

        assert_eq!(
            Config::load(&store).unwrap().default_project.as_deref(),
            Some("beta")
        );
    }

    #[test]
    fn set_default_unknown_slug_errors() {
        let (_tmp, store) = store();
        add(&store, "Alpha").unwrap();
        assert!(matches!(
            set_default(&store, "ghost"),
            Err(Error::NotFound { .. })
        ));
    }

    #[test]
    fn resolve_default_keeps_a_valid_default() {
        let (_tmp, store) = store();
        add(&store, "Alpha").unwrap(); // default = alpha
        add(&store, "Beta").unwrap();

        assert_eq!(resolve_default(&store).unwrap().as_deref(), Some("alpha"));
        // Unchanged, still persisted as alpha.
        assert_eq!(
            Config::load(&store).unwrap().default_project.as_deref(),
            Some("alpha")
        );
    }

    #[test]
    fn resolve_default_heals_dangling_default_to_first_by_name() {
        let (_tmp, store) = store();
        add(&store, "Zebra").unwrap(); // default = zebra
        add(&store, "Alpha").unwrap();
        set_default(&store, "zebra").unwrap();

        // Project deleted out-of-band (by hand), leaving a dangling default.
        std::fs::remove_dir_all(store.project_dir("zebra")).unwrap();

        assert_eq!(resolve_default(&store).unwrap().as_deref(), Some("alpha"));
        assert_eq!(
            Config::load(&store).unwrap().default_project.as_deref(),
            Some("alpha")
        );
    }

    #[test]
    fn resolve_default_heals_none_while_projects_exist() {
        let (_tmp, store) = store();
        add(&store, "Alpha").unwrap();
        Config {
            default_project: None,
        }
        .save(&store)
        .unwrap();

        assert_eq!(resolve_default(&store).unwrap().as_deref(), Some("alpha"));
    }

    #[test]
    fn resolve_default_is_none_without_projects() {
        let (_tmp, store) = store();
        assert_eq!(resolve_default(&store).unwrap(), None);
    }
}
