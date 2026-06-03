use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::slug::slugify;
use crate::store::{Store, atomic_write, now_rfc3339};

/// Folders that always exist in a project and cannot be added, removed, or
/// renamed. `trash` holds archived items.
pub const RESERVED: [&str; 3] = ["main", "pinned", "trash"];

/// A dir: a flat folder inside a project, at `~/.mustr/projects/<p>/<slug>/`.
///
/// Like a project, the slug is the folder name (derived, not stored); the
/// manifest holds only `id` and `created_at`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Dir {
    /// Stable identifier (uuid v7). Survives renames.
    pub id: String,
    /// Folder name; the dir's identity. Derived from the directory.
    #[serde(skip)]
    pub slug: String,
    /// Creation time, RFC3339.
    pub created_at: String,
}

/// Creates the reserved dirs (`main`, `pinned`, `trash`) if missing. Idempotent;
/// used on project creation and to self-heal a project whose reserved folders
/// were deleted by hand.
pub fn ensure_defaults(store: &Store, project: &str) -> Result<()> {
    ensure_project(store, project)?;
    for name in RESERVED {
        if !store.dir_manifest_path(project, name).is_file() {
            create(store, project, name)?;
        }
    }
    Ok(())
}

/// Creates a dir. `input` is slugified into the folder name.
pub fn add(store: &Store, project: &str, input: &str) -> Result<Dir> {
    ensure_project(store, project)?;
    let slug = slugify(input);
    if slug.is_empty() {
        return Err(Error::InvalidName {
            name: input.to_string(),
        });
    }
    if is_reserved(&slug) {
        return Err(Error::Reserved { slug });
    }
    if store.dir_path(project, &slug).exists() {
        return Err(Error::AlreadyExists { kind: "dir", slug });
    }
    create(store, project, &slug)
}

/// Lists a project's dirs: `main` and `pinned` first, then the rest by slug.
pub fn list(store: &Store, project: &str) -> Result<Vec<Dir>> {
    ensure_defaults(store, project)?;

    let project_dir = store.project_dir(project);
    let mut dirs = Vec::new();
    for entry in std::fs::read_dir(&project_dir).map_err(|e| Error::io(&project_dir, e))? {
        let entry = entry.map_err(|e| Error::io(&project_dir, e))?;
        let Some(slug) = entry.file_name().to_str().map(str::to_string) else {
            continue;
        };
        if store.dir_manifest_path(project, &slug).is_file() {
            dirs.push(read_manifest(store, project, &slug)?);
        }
    }
    dirs.sort_by(|a, b| {
        reserved_rank(&a.slug)
            .cmp(&reserved_rank(&b.slug))
            .then(a.slug.cmp(&b.slug))
    });
    Ok(dirs)
}

/// Removes a dir by slug. Reserved dirs cannot be removed.
pub fn remove(store: &Store, project: &str, slug: &str) -> Result<()> {
    ensure_project(store, project)?;
    if is_reserved(slug) {
        return Err(Error::Reserved {
            slug: slug.to_string(),
        });
    }
    let dir = store.dir_path(project, slug);
    if !dir.exists() {
        return Err(Error::NotFound {
            kind: "dir",
            slug: slug.to_string(),
        });
    }
    std::fs::remove_dir_all(&dir).map_err(|e| Error::io(&dir, e))?;
    Ok(())
}

/// Renames a dir (renames its folder). Reserved dirs cannot be the source or
/// target. Preserves `id` and `created_at`.
pub fn rename(store: &Store, project: &str, slug: &str, new: &str) -> Result<Dir> {
    ensure_project(store, project)?;
    if is_reserved(slug) {
        return Err(Error::Reserved {
            slug: slug.to_string(),
        });
    }
    if !store.dir_path(project, slug).exists() {
        return Err(Error::NotFound {
            kind: "dir",
            slug: slug.to_string(),
        });
    }
    let new_slug = slugify(new);
    if new_slug.is_empty() {
        return Err(Error::InvalidName {
            name: new.to_string(),
        });
    }
    if is_reserved(&new_slug) {
        return Err(Error::Reserved { slug: new_slug });
    }
    if new_slug == slug {
        return read_manifest(store, project, slug);
    }
    let new_dir = store.dir_path(project, &new_slug);
    if new_dir.exists() {
        return Err(Error::AlreadyExists {
            kind: "dir",
            slug: new_slug,
        });
    }
    std::fs::rename(store.dir_path(project, slug), &new_dir).map_err(|e| Error::io(&new_dir, e))?;
    read_manifest(store, project, &new_slug)
}

fn is_reserved(slug: &str) -> bool {
    RESERVED.contains(&slug)
}

/// Sort rank: reserved dirs first in their declared order, everything else after.
fn reserved_rank(slug: &str) -> usize {
    RESERVED
        .iter()
        .position(|r| *r == slug)
        .unwrap_or(RESERVED.len())
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

fn create(store: &Store, project: &str, slug: &str) -> Result<Dir> {
    let dir = Dir {
        id: uuid::Uuid::now_v7().to_string(),
        slug: slug.to_string(),
        created_at: now_rfc3339(),
    };
    let path = store.dir_path(project, slug);
    std::fs::create_dir_all(&path).map_err(|e| Error::io(&path, e))?;
    write_manifest(store, project, &dir)?;
    Ok(dir)
}

fn read_manifest(store: &Store, project: &str, slug: &str) -> Result<Dir> {
    let path = store.dir_manifest_path(project, slug);
    let raw = match std::fs::read_to_string(&path) {
        Ok(raw) => raw,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err(Error::NotFound {
                kind: "dir",
                slug: slug.to_string(),
            });
        }
        Err(e) => return Err(Error::io(&path, e)),
    };
    let mut dir: Dir = toml::from_str(&raw).map_err(|source| Error::TomlRead { path, source })?;
    dir.slug = slug.to_string();
    Ok(dir)
}

fn write_manifest(store: &Store, project: &str, dir: &Dir) -> Result<()> {
    let path = store.dir_manifest_path(project, &dir.slug);
    let raw = toml::to_string_pretty(dir).map_err(|source| Error::TomlWrite {
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

    fn project_store() -> (tempfile::TempDir, Store) {
        let tmp = tempfile::tempdir().unwrap();
        let store = Store::new(tmp.path());
        store.ensure().unwrap();
        crate::project::add(&store, "proj").unwrap();
        (tmp, store)
    }

    fn slugs(store: &Store) -> Vec<String> {
        list(store, "proj")
            .unwrap()
            .into_iter()
            .map(|d| d.slug)
            .collect()
    }

    #[test]
    fn new_project_has_reserved_dirs() {
        let (_tmp, store) = project_store();
        assert_eq!(slugs(&store), vec!["main", "pinned", "trash"]);
    }

    #[test]
    fn add_creates_folder_and_manifest() {
        let (_tmp, store) = project_store();

        let dir = add(&store, "proj", "Special Notes").unwrap();

        assert_eq!(dir.slug, "special-notes");
        assert!(!dir.id.is_empty());
        assert!(OffsetDateTime::parse(&dir.created_at, &Rfc3339).is_ok());
        assert!(store.dir_manifest_path("proj", "special-notes").is_file());
    }

    #[test]
    fn add_duplicate_errors() {
        let (_tmp, store) = project_store();
        add(&store, "proj", "abc").unwrap();
        assert!(matches!(
            add(&store, "proj", "abc"),
            Err(Error::AlreadyExists { kind: "dir", .. })
        ));
    }

    #[test]
    fn add_reserved_name_errors() {
        let (_tmp, store) = project_store();
        assert!(matches!(
            add(&store, "proj", "main"),
            Err(Error::Reserved { .. })
        ));
        assert!(matches!(
            add(&store, "proj", "pinned"),
            Err(Error::Reserved { .. })
        ));
        assert!(matches!(
            add(&store, "proj", "trash"),
            Err(Error::Reserved { .. })
        ));
    }

    #[test]
    fn add_empty_slug_is_invalid_name() {
        let (_tmp, store) = project_store();
        assert!(matches!(
            add(&store, "proj", "!!!"),
            Err(Error::InvalidName { .. })
        ));
    }

    #[test]
    fn add_in_unknown_project_errors() {
        let (_tmp, store) = project_store();
        assert!(matches!(
            add(&store, "ghost", "abc"),
            Err(Error::NotFound {
                kind: "project",
                ..
            })
        ));
    }

    #[test]
    fn list_orders_reserved_first_then_by_slug() {
        let (_tmp, store) = project_store();
        add(&store, "proj", "zeta").unwrap();
        add(&store, "proj", "alpha").unwrap();
        assert_eq!(
            slugs(&store),
            vec!["main", "pinned", "trash", "alpha", "zeta"]
        );
    }

    #[test]
    fn list_ignores_entries_without_manifest() {
        let (_tmp, store) = project_store();
        std::fs::create_dir(store.dir_path("proj", "stray")).unwrap();
        assert_eq!(slugs(&store), vec!["main", "pinned", "trash"]);
    }

    #[test]
    fn remove_deletes_the_folder() {
        let (_tmp, store) = project_store();
        add(&store, "proj", "abc").unwrap();

        remove(&store, "proj", "abc").unwrap();

        assert!(!store.dir_path("proj", "abc").exists());
        assert_eq!(slugs(&store), vec!["main", "pinned", "trash"]);
    }

    #[test]
    fn remove_reserved_errors() {
        let (_tmp, store) = project_store();
        assert!(matches!(
            remove(&store, "proj", "main"),
            Err(Error::Reserved { .. })
        ));
    }

    #[test]
    fn remove_unknown_errors() {
        let (_tmp, store) = project_store();
        assert!(matches!(
            remove(&store, "proj", "ghost"),
            Err(Error::NotFound { kind: "dir", .. })
        ));
    }

    #[test]
    fn rename_moves_folder_and_preserves_identity() {
        let (_tmp, store) = project_store();
        let original = add(&store, "proj", "abc").unwrap();

        let renamed = rename(&store, "proj", "abc", "Super Subproject").unwrap();

        assert_eq!(renamed.slug, "super-subproject");
        assert_eq!(renamed.id, original.id);
        assert_eq!(renamed.created_at, original.created_at);
        assert!(!store.dir_path("proj", "abc").exists());
        assert!(
            store
                .dir_manifest_path("proj", "super-subproject")
                .is_file()
        );
    }

    #[test]
    fn rename_reserved_source_or_target_errors() {
        let (_tmp, store) = project_store();
        add(&store, "proj", "abc").unwrap();
        assert!(matches!(
            rename(&store, "proj", "main", "other"),
            Err(Error::Reserved { .. })
        ));
        assert!(matches!(
            rename(&store, "proj", "abc", "main"),
            Err(Error::Reserved { .. })
        ));
    }

    #[test]
    fn rename_to_taken_slug_errors() {
        let (_tmp, store) = project_store();
        add(&store, "proj", "abc").unwrap();
        add(&store, "proj", "def").unwrap();
        assert!(matches!(
            rename(&store, "proj", "abc", "def"),
            Err(Error::AlreadyExists { kind: "dir", .. })
        ));
    }

    #[test]
    fn rename_unknown_errors() {
        let (_tmp, store) = project_store();
        assert!(matches!(
            rename(&store, "proj", "ghost", "x"),
            Err(Error::NotFound { kind: "dir", .. })
        ));
    }

    #[test]
    fn rename_to_empty_slug_is_invalid_name() {
        let (_tmp, store) = project_store();
        add(&store, "proj", "abc").unwrap();
        assert!(matches!(
            rename(&store, "proj", "abc", "!!!"),
            Err(Error::InvalidName { .. })
        ));
    }

    #[test]
    fn ensure_defaults_recreates_hand_deleted_main() {
        let (_tmp, store) = project_store();
        std::fs::remove_dir_all(store.dir_path("proj", "main")).unwrap();

        ensure_defaults(&store, "proj").unwrap();

        assert!(store.dir_manifest_path("proj", "main").is_file());
    }
}
