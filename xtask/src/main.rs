//! Project automation, invoked via the `cargo xtask <task>` alias.

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

/// Name of the installed binary.
const BIN: &str = "mustr";

fn main() -> ExitCode {
    match std::env::args().nth(1).as_deref() {
        Some("install") => report(install()),
        Some(other) => {
            eprintln!("xtask: unknown task '{other}' (available: install)");
            ExitCode::FAILURE
        }
        None => {
            eprintln!("xtask: usage: cargo xtask install");
            ExitCode::FAILURE
        }
    }
}

fn report(result: Result<(), String>) -> ExitCode {
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("xtask: {e}");
            ExitCode::FAILURE
        }
    }
}

/// Runs the test suite, builds a release binary, and installs it into `~/bin`.
/// Tests run first so a failing suite blocks the install.
fn install() -> Result<(), String> {
    let root = workspace_root();
    let home = home_dir()?;
    let manifest = root.join("Cargo.toml");

    run(
        "cargo",
        &["test", "--manifest-path", &lossy(&manifest), "-p", BIN],
    )?;
    run(
        "cargo",
        &[
            "install",
            "--path",
            &lossy(&root),
            "--root",
            &lossy(&home),
            "--force",
        ],
    )?;

    println!("installed {BIN} -> {}", lossy(&install_bin_path(&home)));
    Ok(())
}

/// Path the binary is installed to: `<home>/bin/<BIN>`. `cargo install --root
/// <home>` places binaries under `<home>/bin`.
fn install_bin_path(home: &Path) -> PathBuf {
    home.join("bin").join(BIN)
}

/// Workspace root — the parent of this xtask crate.
fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask crate always has a parent (the workspace root)")
        .to_path_buf()
}

fn home_dir() -> Result<PathBuf, String> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| "HOME environment variable is not set".to_string())
}

fn lossy(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn run(program: &str, args: &[&str]) -> Result<(), String> {
    eprintln!("» {program} {}", args.join(" "));
    let status = Command::new(program)
        .args(args)
        .status()
        .map_err(|e| format!("failed to spawn `{program}`: {e}"))?;
    if !status.success() {
        return Err(format!("`{program}` exited with {status}"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn install_bin_path_is_home_bin_binary() {
        assert_eq!(
            install_bin_path(Path::new("/Users/example")),
            Path::new("/Users/example/bin/mustr")
        );
    }
}
