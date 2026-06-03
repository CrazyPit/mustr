use std::path::PathBuf;

use assert_cmd::Command;
use predicates::prelude::PredicateBooleanExt;
use predicates::str::contains;
use tempfile::TempDir;

/// A `mustr` invocation rooted at a throwaway data dir. `MUSTR_ROOT` points at a
/// path *inside* the temp dir that does not exist yet, so tests can assert the
/// tool creates it. `NO_COLOR` keeps output stable for assertions.
struct Cli {
    _tmp: TempDir,
    root: PathBuf,
    /// Working directory for spawned commands — drives the cwd-derived context.
    cwd: Option<PathBuf>,
}

impl Cli {
    fn new() -> Self {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join(".mustr");
        Cli {
            _tmp: tmp,
            root,
            cwd: None,
        }
    }

    fn cmd(&self) -> Command {
        let mut cmd = Command::cargo_bin("mustr").unwrap();
        cmd.env("MUSTR_ROOT", &self.root).env("NO_COLOR", "1");
        if let Some(cwd) = &self.cwd {
            cmd.current_dir(cwd);
        }
        cmd
    }

    /// A directory outside the data root, for use as a source.
    fn ext_dir(&self, name: &str) -> PathBuf {
        let p = self._tmp.path().join("ext").join(name);
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    /// A git repo (with one commit on `branch`) outside the data root.
    fn ext_git(&self, name: &str, branch: &str) -> PathBuf {
        let p = self.ext_dir(name);
        let git = |args: &[&str]| {
            let st = std::process::Command::new("git")
                .current_dir(&p)
                .args(args)
                .status()
                .unwrap();
            assert!(st.success(), "git {args:?} failed");
        };
        git(&["init", "-q"]);
        git(&["symbolic-ref", "HEAD", &format!("refs/heads/{branch}")]);
        git(&["config", "user.email", "t@example.com"]);
        git(&["config", "user.name", "Test"]);
        std::fs::write(p.join("README"), "x").unwrap();
        git(&["add", "-A"]);
        git(&["commit", "-q", "-m", "init"]);
        p
    }
}

#[test]
fn any_command_initializes_the_data_root() {
    let cli = Cli::new();
    assert!(!cli.root.exists());

    cli.cmd().args(["project", "list"]).assert().success();

    assert!(cli.root.join("projects").is_dir());
}

#[test]
fn project_alias_p_is_equivalent() {
    let cli = Cli::new();
    cli.cmd()
        .args(["project", "add", "Fix Login"])
        .assert()
        .success();

    // The `p` alias and the `ls` alias resolve to the same commands.
    cli.cmd()
        .args(["p", "ls"])
        .assert()
        .success()
        .stdout(contains("fix-login"));
}

#[test]
fn add_reports_creation_and_list_shows_slug() {
    let cli = Cli::new();
    cli.cmd()
        .args(["project", "add", "Fix Login"])
        .assert()
        .success()
        .stdout(contains("fix-login"));

    cli.cmd()
        .args(["project", "list"])
        .assert()
        .success()
        .stdout(contains("fix-login"));
}

#[test]
fn list_marks_current_project_from_cwd() {
    let cli = with_project(); // cwd is inside `proj`
    cli.cmd()
        .args(["project", "list"])
        .assert()
        .success()
        .stdout(contains("★ proj"));
}

#[test]
fn list_empty_shows_hint() {
    let cli = Cli::new();
    cli.cmd()
        .args(["project", "list"])
        .assert()
        .success()
        .stdout(contains("mustr project add"));
}

#[test]
fn rm_with_yes_deletes_without_prompt() {
    let cli = Cli::new();
    cli.cmd()
        .args(["project", "add", "Doomed"])
        .assert()
        .success();

    cli.cmd()
        .args(["project", "rm", "doomed", "--yes"])
        .assert()
        .success();

    cli.cmd()
        .args(["project", "list"])
        .assert()
        .success()
        .stdout(contains("doomed").not());
}

#[test]
fn rm_without_yes_and_no_tty_refuses() {
    let cli = Cli::new();
    cli.cmd()
        .args(["project", "add", "Keep"])
        .assert()
        .success();

    // No TTY (assert_cmd pipes stdin) and no --yes: must refuse, not delete.
    cli.cmd()
        .args(["project", "rm", "keep"])
        .assert()
        .failure()
        .stderr(contains("--yes"));

    cli.cmd()
        .args(["project", "list"])
        .assert()
        .success()
        .stdout(contains("keep"));
}

#[test]
fn rm_unknown_slug_fails() {
    let cli = Cli::new();
    cli.cmd()
        .args(["project", "rm", "ghost", "--yes"])
        .assert()
        .failure()
        .stderr(contains("not found"));
}

#[test]
fn add_duplicate_fails() {
    let cli = Cli::new();
    cli.cmd()
        .args(["project", "add", "Fix Login"])
        .assert()
        .success();
    cli.cmd()
        .args(["project", "add", "Fix Login"])
        .assert()
        .failure()
        .stderr(contains("already exists"));
}

#[test]
fn add_invalid_name_fails() {
    let cli = Cli::new();
    cli.cmd()
        .args(["project", "add", "!!!"])
        .assert()
        .failure()
        .stderr(contains("valid"));
}

#[test]
fn rename_changes_slug() {
    let cli = Cli::new();
    cli.cmd()
        .args(["project", "add", "Fix Login"])
        .assert()
        .success();

    cli.cmd()
        .args(["project", "rename", "fix-login", "Login Fixes"])
        .assert()
        .success();

    cli.cmd()
        .args(["project", "list"])
        .assert()
        .success()
        .stdout(contains("login-fixes").and(contains("fix-login").not()));
}

#[test]
fn dir_list_shows_reserved_folders() {
    let cli = with_project();

    cli.cmd()
        .args(["dir", "list"])
        .assert()
        .success()
        .stdout(contains("main").and(contains("pinned")));
}

#[test]
fn dir_add_then_list() {
    let cli = with_project();

    cli.cmd().args(["dir", "add", "Notes"]).assert().success();

    cli.cmd()
        .args(["d", "ls"])
        .assert()
        .success()
        .stdout(contains("notes"));
}

#[test]
fn dir_rm_reserved_fails() {
    let cli = with_project();

    cli.cmd()
        .args(["dir", "rm", "main", "--yes"])
        .assert()
        .failure()
        .stderr(contains("reserved"));
}

#[test]
fn dir_rm_with_yes_deletes() {
    let cli = with_project();
    cli.cmd().args(["dir", "add", "scratch"]).assert().success();

    cli.cmd()
        .args(["dir", "rm", "scratch", "--yes"])
        .assert()
        .success();

    cli.cmd()
        .args(["dir", "list"])
        .assert()
        .success()
        .stdout(contains("scratch").not());
}

#[test]
fn dir_rename_changes_slug() {
    let cli = with_project();
    cli.cmd().args(["dir", "add", "abc"]).assert().success();

    cli.cmd()
        .args(["dir", "rename", "abc", "Super Subproject"])
        .assert()
        .success();

    cli.cmd()
        .args(["dir", "list"])
        .assert()
        .success()
        .stdout(contains("super-subproject").and(contains("abc").not()));
}

#[test]
fn dir_project_flag_targets_another_project() {
    let cli = Cli::new();
    cli.cmd()
        .args(["project", "add", "alpha"])
        .assert()
        .success(); // default
    cli.cmd()
        .args(["project", "add", "beta"])
        .assert()
        .success();

    // Add to beta via the flag, placed after the subcommand.
    cli.cmd()
        .args(["dir", "add", "extradir", "-p", "beta"])
        .assert()
        .success();

    cli.cmd()
        .args(["dir", "ls", "--project", "beta"])
        .assert()
        .success()
        .stdout(contains("extradir"));

    // The other project (alpha) does not have it.
    cli.cmd()
        .args(["dir", "ls", "-p", "alpha"])
        .assert()
        .success()
        .stdout(contains("extradir").not());
}

#[test]
fn project_mv_alias_renames() {
    let cli = Cli::new();
    cli.cmd().args(["project", "add", "old"]).assert().success();

    cli.cmd().args(["p", "mv", "old", "new"]).assert().success();

    cli.cmd()
        .args(["project", "list"])
        .assert()
        .success()
        .stdout(contains("new").and(contains("old").not()));
}

#[test]
fn dir_mv_alias_renames() {
    let cli = with_project();
    cli.cmd().args(["dir", "add", "old"]).assert().success();

    cli.cmd().args(["d", "mv", "old", "new"]).assert().success();

    cli.cmd()
        .args(["dir", "list"])
        .assert()
        .success()
        .stdout(contains("new").and(contains("old").not()));
}

#[test]
fn dir_without_any_project_fails() {
    let cli = Cli::new();
    cli.cmd()
        .args(["dir", "list"])
        .assert()
        .failure()
        .stderr(contains("project"));
}

/// Creates a project `proj` and runs later commands from inside it, so the
/// cwd-derived context resolves the project without an explicit `--project`.
fn with_project() -> Cli {
    let mut cli = Cli::new();
    cli.cmd()
        .args(["project", "add", "proj"])
        .assert()
        .success();
    cli.cwd = Some(cli.root.join("projects").join("proj"));
    cli
}

#[test]
fn ws_add_lists_with_description_and_prefix() {
    let cli = with_project();
    cli.cmd()
        .args(["w", "add", "tb-123", "-d", "Fix bug"])
        .assert()
        .success();

    // Single dir: no prefix.
    cli.cmd()
        .args(["w", "ls", "main"])
        .assert()
        .success()
        .stdout(
            contains("tb-123")
                .and(contains("Fix bug"))
                .and(contains("main/tb-123").not()),
        );

    // All dirs: prefixed.
    cli.cmd()
        .args(["w", "ls"])
        .assert()
        .success()
        .stdout(contains("main/tb-123"));
}

#[test]
fn ws_rm_soft_moves_to_trash() {
    let cli = with_project();
    cli.cmd().args(["w", "add", "x"]).assert().success();

    cli.cmd().args(["w", "rm", "x"]).assert().success();

    cli.cmd()
        .args(["w", "ls", "trash"])
        .assert()
        .success()
        .stdout(contains("x"));
    cli.cmd()
        .args(["w", "ls", "main"])
        .assert()
        .success()
        .stdout(contains("x").not());
}

#[test]
fn ws_rm_trash_permanent_with_yes() {
    let cli = with_project();
    cli.cmd().args(["w", "add", "x"]).assert().success();
    cli.cmd().args(["w", "rm", "x"]).assert().success(); // -> trash/x

    cli.cmd()
        .args(["w", "rm", "trash/x", "--yes"])
        .assert()
        .success();
    cli.cmd()
        .args(["w", "ls", "trash"])
        .assert()
        .success()
        .stdout(contains("no matching").or(contains("x").not()));
}

#[test]
fn ws_rm_permanent_without_confirm_refuses() {
    let cli = with_project();
    cli.cmd().args(["w", "add", "y"]).assert().success();

    // --permanent needs confirmation; no TTY and no --force/--yes -> refuse.
    cli.cmd()
        .args(["w", "rm", "y", "--permanent"])
        .assert()
        .failure()
        .stderr(contains("--yes"));
}

#[test]
fn ws_rm_auto_suffix_is_reported() {
    let cli = with_project();
    cli.cmd().args(["w", "add", "dup"]).assert().success();
    cli.cmd().args(["w", "rm", "dup"]).assert().success(); // trash/dup
    cli.cmd().args(["w", "add", "dup"]).assert().success();

    cli.cmd()
        .args(["w", "rm", "dup"])
        .assert()
        .success()
        .stdout(contains("dup-2").and(contains("name was taken")));
}

#[test]
fn ws_mv_between_dirs() {
    let cli = with_project();
    cli.cmd().args(["w", "add", "z"]).assert().success();

    cli.cmd()
        .args(["w", "mv", "z", "pinned"])
        .assert()
        .success();

    cli.cmd()
        .args(["w", "ls", "pinned"])
        .assert()
        .success()
        .stdout(contains("z"));
}

#[test]
fn ws_rename_slug_and_description() {
    let cli = with_project();
    cli.cmd()
        .args(["w", "add", "a", "-d", "old"])
        .assert()
        .success();

    cli.cmd().args(["w", "rename", "a", "b"]).assert().success();
    cli.cmd()
        .args(["w", "rename", "b", "-d", "new desc"])
        .assert()
        .success();

    cli.cmd()
        .args(["w", "ls", "main"])
        .assert()
        .success()
        .stdout(
            contains("b")
                .and(contains("new desc"))
                .and(contains("\na ").not()),
        );
}

#[test]
fn ws_grep_finds_by_description_with_prefix() {
    let cli = with_project();
    cli.cmd()
        .args(["w", "add", "login", "-d", "Fix incognito mode"])
        .assert()
        .success();

    cli.cmd()
        .args(["w", "grep", "INCOG"])
        .assert()
        .success()
        .stdout(contains("main/login"));
}

#[test]
fn ws_purge_empties_trash() {
    let cli = with_project();
    cli.cmd().args(["w", "add", "p1"]).assert().success();
    cli.cmd().args(["w", "rm", "p1"]).assert().success(); // -> trash

    cli.cmd().args(["w", "purge", "--yes"]).assert().success();

    cli.cmd()
        .args(["w", "ls", "trash"])
        .assert()
        .success()
        .stdout(contains("p1").not());
}

#[test]
fn path_prints_workspace_directory() {
    let cli = with_project();
    cli.cmd().args(["w", "add", "tb-123"]).assert().success();

    let expected = cli
        .root
        .join("projects")
        .join("proj")
        .join("main")
        .join("tb-123");
    cli.cmd()
        .args(["path", "tb-123"])
        .assert()
        .success()
        .stdout(contains(expected.to_str().unwrap()));

    cli.cmd()
        .args(["path", "ghost"])
        .assert()
        .failure()
        .stderr(contains("not found"));
}

#[test]
fn ws_ls_hides_trash_unless_all_or_trash_flag() {
    let cli = with_project();
    cli.cmd().args(["w", "add", "x"]).assert().success();
    cli.cmd().args(["w", "rm", "x"]).assert().success(); // -> trash/x

    // Default all-dirs listing excludes trash.
    cli.cmd()
        .args(["w", "ls"])
        .assert()
        .success()
        .stdout(contains("trash/x").not());

    // --all and --trash both reveal it.
    cli.cmd()
        .args(["w", "ls", "--all"])
        .assert()
        .success()
        .stdout(contains("trash/x"));
    cli.cmd()
        .args(["w", "ls", "--trash"])
        .assert()
        .success()
        .stdout(contains("trash/x"));
}

#[test]
fn source_add_dir_then_list() {
    let cli = with_project();
    let dir = cli.ext_dir("mylib");

    cli.cmd()
        .args(["src", "add-dir", dir.to_str().unwrap(), "lib"])
        .assert()
        .success();

    cli.cmd()
        .args(["source", "ls"])
        .assert()
        .success()
        .stdout(contains("lib").and(contains("dir")));
}

#[test]
fn source_add_git_detects_branch() {
    let cli = with_project();
    let repo = cli.ext_git("backend", "main");

    cli.cmd()
        .args(["src", "add-git", repo.to_str().unwrap()])
        .assert()
        .success();

    cli.cmd().args(["src", "ls"]).assert().success().stdout(
        contains("backend")
            .and(contains("git"))
            .and(contains("main")),
    );
}

#[test]
fn source_rm_removes_entry() {
    let cli = with_project();
    let dir = cli.ext_dir("mylib");
    cli.cmd()
        .args(["src", "add-dir", dir.to_str().unwrap(), "lib"])
        .assert()
        .success();

    cli.cmd().args(["src", "rm", "lib"]).assert().success();

    cli.cmd()
        .args(["src", "ls"])
        .assert()
        .success()
        .stdout(contains("lib").not());
}

#[test]
fn source_mv_renames() {
    let cli = with_project();
    let dir = cli.ext_dir("mylib");
    cli.cmd()
        .args(["src", "add-dir", dir.to_str().unwrap(), "a"])
        .assert()
        .success();

    cli.cmd().args(["src", "mv", "a", "b"]).assert().success();

    cli.cmd()
        .args(["src", "ls"])
        .assert()
        .success()
        .stdout(contains("b").and(contains("\n  a ").not()));
}

#[test]
fn source_rm_unknown_fails() {
    let cli = with_project();
    cli.cmd()
        .args(["src", "rm", "ghost"])
        .assert()
        .failure()
        .stderr(contains("not found"));
}

#[test]
fn ws_src_add_git_creates_worktree_and_lists() {
    let cli = with_project();
    let repo = cli.ext_git("backend", "main");
    cli.cmd()
        .args(["src", "add-git", repo.to_str().unwrap(), "backend"])
        .assert()
        .success();
    cli.cmd().args(["w", "add", "tb-1"]).assert().success();

    cli.cmd()
        .args(["w", "src", "add", "backend", "-w", "tb-1"])
        .assert()
        .success();

    cli.cmd()
        .args(["w", "src", "ls", "-w", "tb-1"])
        .assert()
        .success()
        .stdout(
            contains("backend")
                .and(contains("worktree"))
                .and(contains("tb-1")),
        );
}

#[test]
fn ws_src_add_all_and_rm() {
    let cli = with_project();
    let repo = cli.ext_git("backend", "main");
    let lib = cli.ext_dir("lib");
    cli.cmd()
        .args(["src", "add-git", repo.to_str().unwrap(), "backend"])
        .assert()
        .success();
    cli.cmd()
        .args(["src", "add-dir", lib.to_str().unwrap(), "lib"])
        .assert()
        .success();
    cli.cmd().args(["w", "add", "tb-1"]).assert().success();

    cli.cmd()
        .args(["w", "src", "add", "--all", "-w", "tb-1"])
        .assert()
        .success();
    cli.cmd()
        .args(["w", "src", "ls", "-w", "tb-1"])
        .assert()
        .success()
        .stdout(contains("backend").and(contains("lib")));

    cli.cmd()
        .args(["w", "src", "rm", "lib", "-w", "tb-1", "-f"])
        .assert()
        .success();
    cli.cmd()
        .args(["w", "src", "ls", "-w", "tb-1"])
        .assert()
        .success()
        .stdout(contains("lib").not());
}

#[test]
fn ws_src_infers_workspace_from_cwd() {
    let mut cli = with_project();
    let lib = cli.ext_dir("lib");
    cli.cmd()
        .args(["src", "add-dir", lib.to_str().unwrap(), "lib"])
        .assert()
        .success();
    cli.cmd().args(["w", "add", "tb-1"]).assert().success();

    // cd into the workspace; no -w needed.
    cli.cwd = Some(
        cli.root
            .join("projects")
            .join("proj")
            .join("main")
            .join("tb-1"),
    );
    cli.cmd()
        .args(["w", "src", "add", "lib"])
        .assert()
        .success();
    cli.cmd()
        .args(["w", "src", "ls"])
        .assert()
        .success()
        .stdout(contains("lib").and(contains("link")));
}

#[test]
fn agent_open_unknown_kind_fails() {
    let mut cli = with_project();
    cli.cmd().args(["w", "add", "tb-1"]).assert().success();
    cli.cwd = Some(
        cli.root
            .join("projects")
            .join("proj")
            .join("main")
            .join("tb-1"),
    );

    cli.cmd()
        .args(["agent", "open", "--type", "opencode"])
        .assert()
        .failure()
        .stderr(contains("unknown agent"));
}

#[test]
fn agent_open_without_workspace_fails() {
    let cli = with_project(); // cwd is the project, not a workspace
    cli.cmd()
        .args(["agent", "open"])
        .assert()
        .failure()
        .stderr(contains("workspace"));
}

#[test]
fn agent_open_alerts_when_already_running() {
    let mut cli = with_project();
    cli.cmd().args(["w", "add", "tb-1"]).assert().success();
    let ws_dir = cli
        .root
        .join("projects")
        .join("proj")
        .join("main")
        .join("tb-1");
    cli.cwd = Some(ws_dir.clone());

    write_agent(&ws_dir, "main", "test-sid");
    // A live pid lock means the agent is running.
    std::fs::write(
        ws_dir.join("agents").join("main.lock"),
        std::process::id().to_string(),
    )
    .unwrap();

    cli.cmd()
        .args(["agent", "open"])
        .assert()
        .failure()
        .stderr(contains("already running"));
}

#[test]
fn unknown_project_flag_reports_project_not_found() {
    let cli = with_project();
    // Wrong --project should blame the project, not something deeper.
    cli.cmd()
        .args(["agent", "open", "-p", "ghost", "-w", "x"])
        .assert()
        .failure()
        .stderr(contains("project 'ghost' not found"));
}

/// Writes a fake agent record so agent ls/rm/rename tests need no real launch.
fn write_agent(ws_dir: &std::path::Path, slug: &str, session_id: &str) {
    std::fs::write(
        ws_dir.join("agents").join(format!("{slug}.toml")),
        format!("id = \"aid\"\nkind = \"claude\"\nsession_id = \"{session_id}\"\ncreated_at = \"2026-06-03T00:00:00Z\"\n"),
    )
    .unwrap();
}

#[test]
fn agent_ls_shows_records_with_status() {
    let mut cli = with_project();
    cli.cmd().args(["w", "add", "tb-1"]).assert().success();
    let ws = cli
        .root
        .join("projects")
        .join("proj")
        .join("main")
        .join("tb-1");
    cli.cwd = Some(ws.clone());
    write_agent(&ws, "main", "sid-1");

    cli.cmd().args(["agent", "ls"]).assert().success().stdout(
        contains("main")
            .and(contains("claude"))
            .and(contains("idle")),
    );
}

#[test]
fn agent_rename_then_rm() {
    let mut cli = with_project();
    cli.cmd().args(["w", "add", "tb-1"]).assert().success();
    let ws = cli
        .root
        .join("projects")
        .join("proj")
        .join("main")
        .join("tb-1");
    cli.cwd = Some(ws.clone());
    write_agent(&ws, "main", "sid-1");

    cli.cmd()
        .args(["agent", "mv", "main", "review"])
        .assert()
        .success();
    cli.cmd()
        .args(["agent", "ls"])
        .assert()
        .success()
        .stdout(contains("review"));

    cli.cmd()
        .args(["agent", "rm", "review", "-f"])
        .assert()
        .success();
    cli.cmd()
        .args(["agent", "ls"])
        .assert()
        .success()
        .stdout(contains("no agents"));
}

#[test]
fn agent_rm_unknown_fails() {
    let mut cli = with_project();
    cli.cmd().args(["w", "add", "tb-1"]).assert().success();
    cli.cwd = Some(
        cli.root
            .join("projects")
            .join("proj")
            .join("main")
            .join("tb-1"),
    );
    cli.cmd()
        .args(["agent", "rm", "ghost", "-f"])
        .assert()
        .failure()
        .stderr(contains("agent 'ghost' not found"));
}
