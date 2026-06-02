use std::error::Error;
use std::io::{IsTerminal, Write};
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use owo_colors::{OwoColorize, Stream};
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

use mustr::project::{self, Project};
use mustr::render::humanize_age;
use mustr::store::Store;

#[derive(Parser)]
#[command(
    name = "mustr",
    version,
    about = "Command center for coding-agent work"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Manage projects
    #[command(alias = "p")]
    Project {
        #[command(subcommand)]
        command: ProjectCommand,
    },
}

#[derive(Subcommand)]
enum ProjectCommand {
    /// Create a new project
    #[command(alias = "new")]
    Add {
        /// Human-facing project name
        name: String,
    },
    /// Remove a project by slug
    #[command(alias = "remove")]
    Rm {
        /// Slug of the project to remove
        slug: String,
        /// Skip the confirmation prompt
        #[arg(short = 'y', long = "yes")]
        yes: bool,
    },
    /// Rename a project by slug
    Rename {
        /// Slug of the project to rename
        slug: String,
        /// New name
        new_name: String,
    },
    /// Select the default project; prints its path to stdout
    #[command(visible_aliases = ["take", "select"])]
    Default {
        /// Slug to select; omit for an interactive picker
        slug: Option<String>,
    },
    /// List projects
    #[command(alias = "ls")]
    List,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

/// Resolves the data root: `MUSTR_ROOT` if set (used by tests), else `~/.mustr`.
fn resolve_root() -> PathBuf {
    if let Some(root) = std::env::var_os("MUSTR_ROOT") {
        return PathBuf::from(root);
    }
    let home = std::env::var_os("HOME").expect("HOME environment variable is not set");
    PathBuf::from(home).join(".mustr")
}

fn run(cli: Cli) -> Result<(), Box<dyn Error>> {
    let store = Store::new(resolve_root());
    store.ensure()?;
    match cli.command {
        Command::Project { command } => run_project(&store, command),
    }
}

fn run_project(store: &Store, command: ProjectCommand) -> Result<(), Box<dyn Error>> {
    match command {
        ProjectCommand::Add { name } => {
            let project = project::add(store, &name)?;
            println!("Created project {} ({})", project.name, project.slug);
        }
        ProjectCommand::Rm { slug, yes } => {
            if !yes && !confirm_removal(&slug)? {
                println!("Aborted.");
                return Ok(());
            }
            project::remove(store, &slug)?;
            println!("Removed project {slug}");
        }
        ProjectCommand::Rename { slug, new_name } => {
            let project = project::rename(store, &slug, &new_name)?;
            println!("Renamed {slug} → {} ({})", project.name, project.slug);
        }
        ProjectCommand::Default { slug } => {
            let slug = match slug {
                Some(slug) => slug,
                None => pick_project(store)?,
            };
            project::set_default(store, &slug)?;
            // Confirmation on stderr, path on stdout, so `cd "$(mustr p default x)"` works.
            eprintln!("selected {slug}");
            println!("{}", store.project_dir(&slug).display());
        }
        ProjectCommand::List => print_list(store)?,
    }
    Ok(())
}

/// Lets the user pick a project interactively. Requires a TTY on stderr (where
/// the picker renders); errors otherwise so non-interactive callers pass a slug.
fn pick_project(store: &Store) -> Result<String, Box<dyn Error>> {
    if !std::io::stderr().is_terminal() {
        return Err("no project given and stderr is not a TTY for interactive selection".into());
    }
    let projects = project::list(store)?;
    if projects.is_empty() {
        return Err("no projects yet — create one with `mustr project add <name>`".into());
    }

    let current = project::resolve_default(store)?;
    let start = current
        .as_deref()
        .and_then(|slug| projects.iter().position(|p| p.slug == slug))
        .unwrap_or(0);
    let labels: Vec<String> = projects
        .iter()
        .map(|p| format!("{} ({})", p.name, p.slug))
        .collect();

    let chosen = dialoguer::Select::new()
        .with_prompt("Select project")
        .items(&labels)
        .default(start)
        .interact()
        .map_err(|e| format!("selection failed: {e}"))?;
    Ok(projects[chosen].slug.clone())
}

/// Confirms a destructive removal. Prompts only when stdin is a TTY; otherwise
/// refuses so non-interactive callers must pass `--yes` explicitly.
fn confirm_removal(slug: &str) -> Result<bool, Box<dyn Error>> {
    if !std::io::stdin().is_terminal() {
        return Err("refusing to delete without --yes (no TTY to confirm)".into());
    }
    print!("Delete project '{slug}'? [y/N] ");
    std::io::stdout().flush()?;
    let mut answer = String::new();
    std::io::stdin().read_line(&mut answer)?;
    Ok(matches!(answer.trim(), "y" | "Y" | "yes"))
}

fn print_list(store: &Store) -> Result<(), Box<dyn Error>> {
    let projects = project::list(store)?;
    let default = project::resolve_default(store)?;

    if projects.is_empty() {
        println!();
        println!("  No projects yet. Create one with `mustr project add <name>`.");
        println!();
        return Ok(());
    }

    let name_width = projects
        .iter()
        .map(|p| p.name.chars().count())
        .max()
        .unwrap_or(0);
    let slug_width = projects
        .iter()
        .map(|p| p.slug.chars().count())
        .max()
        .unwrap_or(0);
    let now = OffsetDateTime::now_utc();

    println!();
    let header = format!("projects · {}", store.projects_dir().display());
    println!(
        "  {}",
        header.if_supports_color(Stream::Stdout, |t| t.dimmed())
    );
    println!();

    for project in &projects {
        let is_default = default.as_deref() == Some(project.slug.as_str());
        let marker = if is_default { "★" } else { " " };
        let name = format!("{:<width$}", project.name, width = name_width);
        let slug = format!("{:<width$}", project.slug, width = slug_width);
        let age = age_label(project, now);

        println!(
            "  {} {}  {}  {}",
            marker.if_supports_color(Stream::Stdout, |t| t.yellow()),
            name.if_supports_color(Stream::Stdout, |t| t.bold()),
            slug.if_supports_color(Stream::Stdout, |t| t.dimmed()),
            age.if_supports_color(Stream::Stdout, |t| t.dimmed()),
        );
    }

    println!();
    let count = format!(
        "{} project{}",
        projects.len(),
        if projects.len() == 1 { "" } else { "s" }
    );
    println!(
        "  {}",
        count.if_supports_color(Stream::Stdout, |t| t.dimmed())
    );
    Ok(())
}

/// Human age of a project from its `created_at`, or empty if it cannot be parsed.
fn age_label(project: &Project, now: OffsetDateTime) -> String {
    match OffsetDateTime::parse(&project.created_at, &Rfc3339) {
        Ok(created) => humanize_age((now - created).whole_seconds()),
        Err(_) => String::new(),
    }
}
