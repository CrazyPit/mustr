//! Materializing project sources into a workspace's `src/`: git sources become
//! worktrees, dir sources become symlinks.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::error::{Error, Result};
use crate::source::{self, SourceKind};
use crate::store::Store;

/// A source materialized into a workspace's `src/`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mount {
    pub slug: String,
    pub kind: MountKind,
}

/// How a source is materialized.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MountKind {
    /// A git worktree checked out on `branch`.
    Worktree { branch: String },
    /// A symlink to an external directory.
    Link { target: PathBuf },
}

/// Materializes source `source_slug` into the workspace. Git → worktree on
/// `branch` (default: the workspace slug); dir → symlink.
pub fn add(
    store: &Store,
    project: &str,
    dir: &str,
    workspace: &str,
    source_slug: &str,
    branch: Option<&str>,
) -> Result<Mount> {
    ensure_workspace(store, project, dir, workspace)?;
    let source = source::get(store, project, source_slug)?;

    let src_dir = store.workspace_src_dir(project, dir, workspace);
    std::fs::create_dir_all(&src_dir).map_err(|e| Error::io(&src_dir, e))?;
    let dest = src_dir.join(source_slug);
    if dest.symlink_metadata().is_ok() {
        return Err(Error::AlreadyExists {
            kind: "source",
            slug: source_slug.to_string(),
        });
    }

    match source.kind {
        SourceKind::Dir => {
            symlink(&source.path, &dest)?;
            Ok(Mount {
                slug: source_slug.to_string(),
                kind: MountKind::Link {
                    target: source.path,
                },
            })
        }
        SourceKind::Git => {
            let branch = branch.unwrap_or(workspace).to_string();
            let base = source.base_branch.as_deref().unwrap_or("HEAD");
            create_worktree(&source.path, &dest, &branch, base)?;
            Ok(Mount {
                slug: source_slug.to_string(),
                kind: MountKind::Worktree { branch },
            })
        }
    }
}

/// Materializes every project source not already present in `src/`.
pub fn add_all(store: &Store, project: &str, dir: &str, workspace: &str) -> Result<Vec<Mount>> {
    ensure_workspace(store, project, dir, workspace)?;
    let src_dir = store.workspace_src_dir(project, dir, workspace);
    let mut added = Vec::new();
    for source in source::list(store, project)? {
        if src_dir.join(&source.slug).symlink_metadata().is_ok() {
            continue;
        }
        added.push(add(store, project, dir, workspace, &source.slug, None)?);
    }
    Ok(added)
}

/// Lists the sources materialized in the workspace's `src/`, sorted by slug.
pub fn list(store: &Store, project: &str, dir: &str, workspace: &str) -> Result<Vec<Mount>> {
    ensure_workspace(store, project, dir, workspace)?;
    let src_dir = store.workspace_src_dir(project, dir, workspace);
    let entries = match std::fs::read_dir(&src_dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(Error::io(&src_dir, e)),
    };

    let mut mounts = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|e| Error::io(&src_dir, e))?;
        let Some(slug) = entry.file_name().to_str().map(str::to_string) else {
            continue;
        };
        let path = entry.path();
        let Ok(meta) = path.symlink_metadata() else {
            continue;
        };
        if meta.is_symlink() {
            let target = std::fs::read_link(&path).unwrap_or_default();
            mounts.push(Mount {
                slug,
                kind: MountKind::Link { target },
            });
        } else if path.join(".git").exists() {
            let branch = git_opt(&path, &["rev-parse", "--abbrev-ref", "HEAD"])
                .unwrap_or_else(|| "?".to_string());
            mounts.push(Mount {
                slug,
                kind: MountKind::Worktree { branch },
            });
        }
    }
    mounts.sort_by(|a, b| a.slug.cmp(&b.slug));
    Ok(mounts)
}

/// Removes a materialized source: unlinks a symlink, or `git worktree remove`s a
/// worktree (with `--force` when `force`).
pub fn remove(
    store: &Store,
    project: &str,
    dir: &str,
    workspace: &str,
    slug: &str,
    force: bool,
) -> Result<()> {
    ensure_workspace(store, project, dir, workspace)?;
    let path = store.workspace_src_dir(project, dir, workspace).join(slug);
    let Ok(meta) = path.symlink_metadata() else {
        return Err(Error::NotFound {
            kind: "source",
            slug: slug.to_string(),
        });
    };

    if meta.is_symlink() {
        std::fs::remove_file(&path).map_err(|e| Error::io(&path, e))?;
    } else if path.join(".git").exists() {
        let path_str = path.to_string_lossy().into_owned();
        let mut args = vec!["worktree", "remove"];
        if force {
            args.push("--force");
        }
        args.push(&path_str);
        git_run(&path, &args)?;
    } else {
        std::fs::remove_dir_all(&path).map_err(|e| Error::io(&path, e))?;
    }
    Ok(())
}

/// Repairs the git links of every worktree in `src/` after the workspace folder
/// moved (re-points the admin `gitdir` back-link). Best-effort per worktree.
pub fn repair_worktrees(store: &Store, project: &str, dir: &str, workspace: &str) -> Result<()> {
    for mount in list(store, project, dir, workspace)? {
        if matches!(mount.kind, MountKind::Worktree { .. }) {
            let path = store
                .workspace_src_dir(project, dir, workspace)
                .join(&mount.slug);
            let _ = git_run(&path, &["worktree", "repair"]);
        }
    }
    Ok(())
}

/// Detaches every worktree in `src/` from its source repo (`git worktree remove
/// --force`), leaving the branch intact. Call before permanently deleting the
/// workspace so source repos aren't left with prunable entries. Best-effort.
pub fn remove_worktrees(store: &Store, project: &str, dir: &str, workspace: &str) -> Result<()> {
    for mount in list(store, project, dir, workspace)? {
        if matches!(mount.kind, MountKind::Worktree { .. }) {
            let path = store
                .workspace_src_dir(project, dir, workspace)
                .join(&mount.slug);
            if let Some(repo) = git_common_repo(&path) {
                let _ = git_run(
                    &repo,
                    &["worktree", "remove", "--force", &path.to_string_lossy()],
                );
            }
        }
    }
    Ok(())
}

/// The main repo of a worktree (parent of its common git dir).
fn git_common_repo(worktree: &Path) -> Option<PathBuf> {
    let common = git_opt(
        worktree,
        &["rev-parse", "--path-format=absolute", "--git-common-dir"],
    )?;
    Path::new(&common).parent().map(Path::to_path_buf)
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

/// Adds a worktree at `dest`: checks out `branch` if it exists, else creates it
/// from `base`.
fn create_worktree(repo: &Path, dest: &Path, branch: &str, base: &str) -> Result<()> {
    let dest = dest.to_string_lossy().into_owned();
    if git_ok(
        repo,
        &[
            "rev-parse",
            "--verify",
            "--quiet",
            &format!("refs/heads/{branch}"),
        ],
    ) {
        git_run(repo, &["worktree", "add", &dest, branch])
    } else {
        git_run(repo, &["worktree", "add", &dest, "-b", branch, base])
    }
}

#[cfg(unix)]
fn symlink(target: &Path, dest: &Path) -> Result<()> {
    std::os::unix::fs::symlink(target, dest).map_err(|e| Error::io(dest, e))
}

#[cfg(not(unix))]
fn symlink(_target: &Path, dest: &Path) -> Result<()> {
    Err(Error::io(
        dest,
        std::io::Error::new(std::io::ErrorKind::Unsupported, "symlinks require unix"),
    ))
}

fn git_run(repo: &Path, args: &[&str]) -> Result<()> {
    let output = Command::new("git")
        .current_dir(repo)
        .args(args)
        .output()
        .map_err(|e| Error::Git {
            message: format!("failed to run git: {e}"),
        })?;
    if output.status.success() {
        Ok(())
    } else {
        Err(Error::Git {
            message: String::from_utf8_lossy(&output.stderr).trim().to_string(),
        })
    }
}

fn git_ok(repo: &Path, args: &[&str]) -> bool {
    Command::new("git")
        .current_dir(repo)
        .args(args)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn git_opt(repo: &Path, args: &[&str]) -> Option<String> {
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

    fn setup() -> (tempfile::TempDir, Store) {
        let tmp = tempfile::tempdir().unwrap();
        let store = Store::new(tmp.path());
        store.ensure().unwrap();
        crate::project::add(&store, "proj").unwrap();
        crate::workspace::add(&store, "proj", "main", "ws", None).unwrap();
        (tmp, store)
    }

    fn run_git(dir: &Path, args: &[&str]) {
        let st = Command::new("git")
            .current_dir(dir)
            .args(args)
            .status()
            .unwrap();
        assert!(st.success(), "git {args:?} failed");
    }

    fn init_repo(parent: &Path, name: &str, branch: &str) -> PathBuf {
        let p = parent.join(name);
        std::fs::create_dir_all(&p).unwrap();
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

    fn make_dir(parent: &Path, name: &str) -> PathBuf {
        let p = parent.join(name);
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    fn src_path(store: &Store, slug: &str) -> PathBuf {
        store.workspace_src_dir("proj", "main", "ws").join(slug)
    }

    fn git_out(dir: &Path, args: &[&str]) -> String {
        let out = Command::new("git")
            .current_dir(dir)
            .args(args)
            .output()
            .unwrap();
        String::from_utf8_lossy(&out.stdout).into_owned()
    }

    #[test]
    fn remove_worktrees_detaches_and_keeps_branch() {
        let (tmp, store) = setup();
        let repo = init_repo(tmp.path(), "backend", "main");
        source::add_git(&store, "proj", repo.to_str().unwrap(), None, None).unwrap();
        add(&store, "proj", "main", "ws", "backend", None).unwrap();
        assert_eq!(
            git_out(&repo, &["worktree", "list", "--porcelain"])
                .matches("worktree ")
                .count(),
            2
        );

        remove_worktrees(&store, "proj", "main", "ws").unwrap();

        // Admin entry gone (only the main worktree left), folder removed, branch kept.
        assert_eq!(
            git_out(&repo, &["worktree", "list", "--porcelain"])
                .matches("worktree ")
                .count(),
            1
        );
        assert!(!src_path(&store, "backend").exists());
        assert!(git_out(&repo, &["branch", "--list", "ws"]).contains("ws"));
    }

    #[test]
    fn repair_worktrees_fixes_link_after_move() {
        let (tmp, store) = setup();
        let repo = init_repo(tmp.path(), "backend", "main");
        source::add_git(&store, "proj", repo.to_str().unwrap(), None, None).unwrap();
        add(&store, "proj", "main", "ws", "backend", None).unwrap();

        // Move the whole workspace folder (as `w mv` would) — breaks the back-link.
        let from = store.workspace_path("proj", "main", "ws");
        let to = store.workspace_path("proj", "pinned", "ws");
        std::fs::rename(&from, &to).unwrap();
        assert!(git_out(&repo, &["worktree", "list"]).contains("prunable"));

        repair_worktrees(&store, "proj", "pinned", "ws").unwrap();

        let listing = git_out(&repo, &["worktree", "list"]);
        assert!(!listing.contains("prunable"));
        assert!(listing.contains("pinned"));
    }

    #[test]
    fn add_git_creates_worktree_on_workspace_branch() {
        let (tmp, store) = setup();
        let repo = init_repo(tmp.path(), "backend", "main");
        source::add_git(&store, "proj", repo.to_str().unwrap(), None, None).unwrap();

        let m = add(&store, "proj", "main", "ws", "backend", None).unwrap();

        assert_eq!(m.slug, "backend");
        assert_eq!(
            m.kind,
            MountKind::Worktree {
                branch: "ws".into()
            }
        );
        assert!(src_path(&store, "backend").join(".git").exists());
    }

    #[test]
    fn add_git_branch_override() {
        let (tmp, store) = setup();
        let repo = init_repo(tmp.path(), "backend", "main");
        source::add_git(&store, "proj", repo.to_str().unwrap(), None, None).unwrap();

        let m = add(&store, "proj", "main", "ws", "backend", Some("feature-x")).unwrap();
        assert_eq!(
            m.kind,
            MountKind::Worktree {
                branch: "feature-x".into()
            }
        );
    }

    #[test]
    fn add_dir_creates_symlink() {
        let (tmp, store) = setup();
        let d = make_dir(tmp.path(), "designs");
        source::add_dir(&store, "proj", d.to_str().unwrap(), Some("designs")).unwrap();

        let m = add(&store, "proj", "main", "ws", "designs", None).unwrap();

        assert_eq!(
            m.kind,
            MountKind::Link {
                target: d.canonicalize().unwrap()
            }
        );
        assert!(src_path(&store, "designs").is_symlink());
    }

    #[test]
    fn add_unknown_source_errors() {
        let (_tmp, store) = setup();
        assert!(matches!(
            add(&store, "proj", "main", "ws", "ghost", None),
            Err(Error::NotFound { kind: "source", .. })
        ));
    }

    #[test]
    fn add_already_materialized_errors() {
        let (tmp, store) = setup();
        let d = make_dir(tmp.path(), "designs");
        source::add_dir(&store, "proj", d.to_str().unwrap(), Some("designs")).unwrap();
        add(&store, "proj", "main", "ws", "designs", None).unwrap();
        assert!(matches!(
            add(&store, "proj", "main", "ws", "designs", None),
            Err(Error::AlreadyExists { kind: "source", .. })
        ));
    }

    #[test]
    fn add_all_materializes_each_source() {
        let (tmp, store) = setup();
        let repo = init_repo(tmp.path(), "backend", "main");
        let d = make_dir(tmp.path(), "designs");
        source::add_git(&store, "proj", repo.to_str().unwrap(), None, None).unwrap();
        source::add_dir(&store, "proj", d.to_str().unwrap(), Some("designs")).unwrap();

        let added = add_all(&store, "proj", "main", "ws").unwrap();
        assert_eq!(added.len(), 2);
        assert_eq!(list(&store, "proj", "main", "ws").unwrap().len(), 2);
    }

    #[test]
    fn list_returns_mounts_sorted_by_slug() {
        let (tmp, store) = setup();
        let d = make_dir(tmp.path(), "zeta");
        let repo = init_repo(tmp.path(), "alpha", "main");
        source::add_dir(&store, "proj", d.to_str().unwrap(), Some("zeta")).unwrap();
        source::add_git(&store, "proj", repo.to_str().unwrap(), None, None).unwrap();
        add(&store, "proj", "main", "ws", "zeta", None).unwrap();
        add(&store, "proj", "main", "ws", "alpha", None).unwrap();

        let slugs: Vec<_> = list(&store, "proj", "main", "ws")
            .unwrap()
            .into_iter()
            .map(|m| m.slug)
            .collect();
        assert_eq!(slugs, vec!["alpha", "zeta"]);
    }

    #[test]
    fn remove_symlink_unlinks_and_keeps_external_dir() {
        let (tmp, store) = setup();
        let d = make_dir(tmp.path(), "designs");
        source::add_dir(&store, "proj", d.to_str().unwrap(), Some("designs")).unwrap();
        add(&store, "proj", "main", "ws", "designs", None).unwrap();

        remove(&store, "proj", "main", "ws", "designs", false).unwrap();

        assert!(!src_path(&store, "designs").exists());
        assert!(src_path(&store, "designs").symlink_metadata().is_err());
        assert!(d.is_dir());
    }

    #[test]
    fn remove_worktree() {
        let (tmp, store) = setup();
        let repo = init_repo(tmp.path(), "backend", "main");
        source::add_git(&store, "proj", repo.to_str().unwrap(), None, None).unwrap();
        add(&store, "proj", "main", "ws", "backend", None).unwrap();

        remove(&store, "proj", "main", "ws", "backend", false).unwrap();
        assert!(!src_path(&store, "backend").exists());
    }

    #[test]
    fn remove_unknown_errors() {
        let (_tmp, store) = setup();
        assert!(matches!(
            remove(&store, "proj", "main", "ws", "ghost", false),
            Err(Error::NotFound { kind: "source", .. })
        ));
    }
}
