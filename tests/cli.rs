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
}

impl Cli {
    fn new() -> Self {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join(".mustr");
        Cli { _tmp: tmp, root }
    }

    fn cmd(&self) -> Command {
        let mut cmd = Command::cargo_bin("mustr").unwrap();
        cmd.env("MUSTR_ROOT", &self.root).env("NO_COLOR", "1");
        cmd
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
        .stdout(contains("Fix Login").and(contains("fix-login")));
}

#[test]
fn add_reports_creation_and_list_shows_default_marker() {
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
        .stdout(contains("★").and(contains("Fix Login")));
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
fn default_moves_the_star() {
    let cli = Cli::new();
    cli.cmd()
        .args(["project", "add", "Alpha"])
        .assert()
        .success();
    cli.cmd()
        .args(["project", "add", "Beta"])
        .assert()
        .success();

    cli.cmd()
        .args(["project", "default", "beta"])
        .assert()
        .success();

    cli.cmd()
        .args(["project", "list"])
        .assert()
        .success()
        .stdout(contains("★ Beta").and(contains("★ Alpha").not()));
}

#[test]
fn default_prints_only_the_path_on_stdout() {
    let cli = Cli::new();
    cli.cmd()
        .args(["project", "add", "Fix Login"])
        .assert()
        .success();

    let expected_path = cli.root.join("projects").join("fix-login");
    cli.cmd()
        .args(["project", "default", "fix-login"])
        .assert()
        .success()
        // The path, and nothing chatty, on stdout — the confirmation is on stderr.
        .stdout(contains(expected_path.to_str().unwrap()).and(contains("selected").not()))
        .stderr(contains("selected"));
}

#[test]
fn take_alias_selects() {
    let cli = Cli::new();
    cli.cmd()
        .args(["project", "add", "Alpha"])
        .assert()
        .success();
    cli.cmd()
        .args(["project", "add", "Beta"])
        .assert()
        .success();

    cli.cmd().args(["p", "take", "beta"]).assert().success();

    cli.cmd()
        .args(["project", "list"])
        .assert()
        .success()
        .stdout(contains("★ Beta"));
}

#[test]
fn default_unknown_slug_fails() {
    let cli = Cli::new();
    cli.cmd()
        .args(["project", "default", "ghost"])
        .assert()
        .failure()
        .stderr(contains("not found"));
}

#[test]
fn default_without_arg_and_no_tty_fails() {
    let cli = Cli::new();
    cli.cmd()
        .args(["project", "add", "Alpha"])
        .assert()
        .success();

    // assert_cmd is not a TTY: the interactive picker cannot run.
    cli.cmd()
        .args(["project", "default"])
        .assert()
        .failure()
        .stderr(contains("TTY"));
}

#[test]
fn list_heals_star_after_manual_deletion_of_default() {
    let cli = Cli::new();
    cli.cmd()
        .args(["project", "add", "Alpha"])
        .assert()
        .success(); // default = alpha
    cli.cmd()
        .args(["project", "add", "Beta"])
        .assert()
        .success();

    // Project folder removed out-of-band, leaving a dangling default.
    std::fs::remove_dir_all(cli.root.join("projects").join("alpha")).unwrap();

    cli.cmd()
        .args(["project", "list"])
        .assert()
        .success()
        .stdout(contains("★ Beta"));
}
