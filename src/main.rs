use std::error::Error;
use std::io::{IsTerminal, Write};
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use owo_colors::{OwoColorize, Stream};
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

use mustr::dir;
use mustr::project;
use mustr::render::humanize_age;
use mustr::slug::slugify;
use mustr::store::Store;
use mustr::workspace::{self, Removal, Workspace};

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
    /// Manage a project's dirs
    #[command(alias = "d")]
    Dir {
        /// Project slug to act on (defaults to the selected project)
        #[arg(short = 'p', long, global = true)]
        project: Option<String>,
        #[command(subcommand)]
        command: DirCommand,
    },
    /// Manage workspaces
    #[command(alias = "w")]
    Workspace {
        /// Project slug to act on (defaults to the selected project)
        #[arg(short = 'p', long, global = true)]
        project: Option<String>,
        #[command(subcommand)]
        command: WorkspaceCommand,
    },
}

#[derive(Subcommand)]
enum WorkspaceCommand {
    /// Create a workspace at [dir/]slug (dir defaults to main)
    #[command(alias = "new")]
    Add {
        /// Address `[dir/]slug`
        address: String,
        /// Optional description
        #[arg(short = 'd', long)]
        description: Option<String>,
    },
    /// Remove a workspace (soft-delete to trash; --force for permanent)
    #[command(alias = "remove")]
    Rm {
        /// Address `[dir/]slug`
        address: String,
        /// Delete permanently instead of moving to trash
        #[arg(short = 'f', long)]
        force: bool,
        /// Skip the confirmation prompt for permanent deletes
        #[arg(short = 'y', long = "yes")]
        yes: bool,
    },
    /// Permanently empty the trash dir
    Purge {
        /// Skip the confirmation prompt
        #[arg(short = 'y', long = "yes")]
        yes: bool,
    },
    /// Rename a workspace and/or set its description
    Rename {
        /// Address `[dir/]slug`
        address: String,
        /// New slug (optional)
        new_slug: Option<String>,
        /// New description (optional)
        #[arg(short = 'd', long)]
        description: Option<String>,
    },
    /// Move a workspace to another dir
    Mv {
        /// Address `[dir/]slug`
        address: String,
        /// Target dir
        target_dir: String,
    },
    /// List workspaces (all dirs, or one dir if given)
    #[command(alias = "ls")]
    List {
        /// Limit to a single dir
        dir: Option<String>,
        /// Include the trash dir
        #[arg(long, visible_alias = "trash")]
        all: bool,
    },
    /// Search workspaces by slug and description (case-insensitive)
    Grep {
        /// Query string
        query: String,
        /// Include the trash dir
        #[arg(long, visible_alias = "trash")]
        all: bool,
    },
}

#[derive(Subcommand)]
enum DirCommand {
    /// Create a new dir
    #[command(alias = "new")]
    Add {
        /// Dir slug (spaces and punctuation are normalized)
        slug: String,
    },
    /// Remove a dir by slug
    #[command(alias = "remove")]
    Rm {
        /// Slug of the dir to remove
        slug: String,
        /// Skip the confirmation prompt
        #[arg(short = 'y', long = "yes")]
        yes: bool,
    },
    /// Rename a dir (renames its folder)
    #[command(alias = "mv")]
    Rename {
        /// Current slug
        slug: String,
        /// New slug (spaces and punctuation are normalized)
        new_slug: String,
    },
    /// List dirs
    #[command(alias = "ls")]
    List,
}

#[derive(Subcommand)]
enum ProjectCommand {
    /// Create a new project
    #[command(alias = "new")]
    Add {
        /// Project slug (spaces and punctuation are normalized)
        slug: String,
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
    /// Rename a project (renames its folder)
    #[command(alias = "mv")]
    Rename {
        /// Current slug
        slug: String,
        /// New slug (spaces and punctuation are normalized)
        new_slug: String,
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
        Command::Dir { project, command } => run_dir(&store, project, command),
        Command::Workspace { project, command } => run_workspace(&store, project, command),
    }
}

fn run_workspace(
    store: &Store,
    project: Option<String>,
    command: WorkspaceCommand,
) -> Result<(), Box<dyn Error>> {
    let project = resolve_project(store, project)?;
    match command {
        WorkspaceCommand::Add {
            address,
            description,
        } => {
            let (dir, slug) = workspace::parse_address(&address);
            let ws = workspace::add(store, &project, &dir, &slug, description)?;
            println!("Created workspace {}/{} in {project}", ws.dir, ws.slug);
        }
        WorkspaceCommand::Rm {
            address,
            force,
            yes,
        } => {
            let (dir, slug) = workspace::parse_address(&address);
            let permanent = force || dir == "trash";
            if permanent
                && !yes
                && !confirm_removal(&format!("workspace '{dir}/{slug}' permanently"))?
            {
                println!("Aborted.");
                return Ok(());
            }
            match workspace::remove(store, &project, &dir, &slug, force)? {
                Removal::Deleted => println!("Deleted workspace {dir}/{slug}"),
                Removal::Trashed { slug: final_slug } if final_slug == slug => {
                    println!("Moved {dir}/{slug} to trash");
                }
                Removal::Trashed { slug: final_slug } => {
                    println!("Moved {dir}/{slug} to trash as {final_slug} (name was taken)");
                }
            }
        }
        WorkspaceCommand::Purge { yes } => {
            if !yes && !confirm_removal("all workspaces in trash")? {
                println!("Aborted.");
                return Ok(());
            }
            let n = workspace::purge(store, &project)?;
            println!("Purged {n} workspace{}", if n == 1 { "" } else { "s" });
        }
        WorkspaceCommand::Rename {
            address,
            new_slug,
            description,
        } => {
            if new_slug.is_none() && description.is_none() {
                return Err("nothing to change — pass a new slug and/or --description".into());
            }
            let (dir, slug) = workspace::parse_address(&address);
            let ws = workspace::rename(
                store,
                &project,
                &dir,
                &slug,
                new_slug.as_deref(),
                description.as_deref(),
            )?;
            println!("Updated {dir}/{}", ws.slug);
        }
        WorkspaceCommand::Mv {
            address,
            target_dir,
        } => {
            let (dir, slug) = workspace::parse_address(&address);
            let final_slug = workspace::move_to_dir(store, &project, &dir, &slug, &target_dir)?;
            let target = slugify(&target_dir);
            if final_slug == slug {
                println!("Moved {dir}/{slug} to {target}");
            } else {
                println!("Moved {dir}/{slug} to {target}/{final_slug} (name was taken)");
            }
        }
        WorkspaceCommand::List { dir, all } => {
            let workspaces = workspace::list(store, &project, dir.as_deref(), all)?;
            let scope = match &dir {
                Some(d) => format!("{project}/{d}"),
                None => project.clone(),
            };
            print_workspaces(&scope, &workspaces, dir.is_none());
        }
        WorkspaceCommand::Grep { query, all } => {
            let workspaces = workspace::grep(store, &project, &query, all)?;
            print_workspaces(&format!("{project} · grep: {query}"), &workspaces, true);
        }
    }
    Ok(())
}

/// Resolves the project a `dir` command acts on: the `--project` slug if given,
/// otherwise the selected default. Errors when no project is available.
fn resolve_project(store: &Store, explicit: Option<String>) -> Result<String, Box<dyn Error>> {
    match explicit {
        Some(slug) => Ok(slug),
        None => project::resolve_default(store)?.ok_or_else(|| {
            "no project selected — create one with `mustr project add <name>`".into()
        }),
    }
}

fn run_dir(
    store: &Store,
    project: Option<String>,
    command: DirCommand,
) -> Result<(), Box<dyn Error>> {
    let project = resolve_project(store, project)?;
    match command {
        DirCommand::Add { slug } => {
            let created = dir::add(store, &project, &slug)?;
            println!("Created dir {} in {project}", created.slug);
        }
        DirCommand::Rm { slug, yes } => {
            if !yes && !confirm_removal(&format!("dir '{slug}' in '{project}'"))? {
                println!("Aborted.");
                return Ok(());
            }
            dir::remove(store, &project, &slug)?;
            println!("Removed dir {slug} from {project}");
        }
        DirCommand::Rename { slug, new_slug } => {
            let renamed = dir::rename(store, &project, &slug, &new_slug)?;
            println!("Renamed {slug} → {} in {project}", renamed.slug);
        }
        DirCommand::List => print_dirs(store, &project)?,
    }
    Ok(())
}

fn run_project(store: &Store, command: ProjectCommand) -> Result<(), Box<dyn Error>> {
    match command {
        ProjectCommand::Add { slug } => {
            let project = project::add(store, &slug)?;
            println!("Created project {}", project.slug);
        }
        ProjectCommand::Rm { slug, yes } => {
            if !yes && !confirm_removal(&format!("project '{slug}'"))? {
                println!("Aborted.");
                return Ok(());
            }
            project::remove(store, &slug)?;
            println!("Removed project {slug}");
        }
        ProjectCommand::Rename { slug, new_slug } => {
            let project = project::rename(store, &slug, &new_slug)?;
            println!("Renamed {slug} → {}", project.slug);
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
    let labels: Vec<String> = projects.iter().map(|p| p.slug.clone()).collect();

    let chosen = dialoguer::Select::new()
        .with_prompt("Select project")
        .items(&labels)
        .default(start)
        .interact()
        .map_err(|e| format!("selection failed: {e}"))?;
    Ok(projects[chosen].slug.clone())
}

/// Confirms a destructive removal of `what`. Prompts only when stdin is a TTY;
/// otherwise refuses so non-interactive callers must pass `--yes` explicitly.
fn confirm_removal(what: &str) -> Result<bool, Box<dyn Error>> {
    if !std::io::stdin().is_terminal() {
        return Err("refusing to delete without --yes (no TTY to confirm)".into());
    }
    print!("Delete {what}? [y/N] ");
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
        let slug = format!("{:<width$}", project.slug, width = slug_width);
        let age = age_label(&project.created_at, now);

        println!(
            "  {} {}  {}",
            marker.if_supports_color(Stream::Stdout, |t| t.yellow()),
            slug.if_supports_color(Stream::Stdout, |t| t.bold()),
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

fn print_dirs(store: &Store, project: &str) -> Result<(), Box<dyn Error>> {
    let dirs = dir::list(store, project)?;
    let slug_width = dirs
        .iter()
        .map(|d| d.slug.chars().count())
        .max()
        .unwrap_or(0);
    let now = OffsetDateTime::now_utc();

    println!();
    let header = format!("dirs · {project}");
    println!(
        "  {}",
        header.if_supports_color(Stream::Stdout, |t| t.dimmed())
    );
    println!();

    for d in &dirs {
        let slug = format!("{:<width$}", d.slug, width = slug_width);
        let age = age_label(&d.created_at, now);
        println!(
            "  {}  {}",
            slug.if_supports_color(Stream::Stdout, |t| t.bold()),
            age.if_supports_color(Stream::Stdout, |t| t.dimmed()),
        );
    }

    println!();
    let count = format!(
        "{} dir{}",
        dirs.len(),
        if dirs.len() == 1 { "" } else { "s" }
    );
    println!(
        "  {}",
        count.if_supports_color(Stream::Stdout, |t| t.dimmed())
    );
    Ok(())
}

fn print_workspaces(scope: &str, workspaces: &[Workspace], show_prefix: bool) {
    println!();
    let header = format!("workspaces · {scope}");
    println!(
        "  {}",
        header.if_supports_color(Stream::Stdout, |t| t.dimmed())
    );
    println!();

    if workspaces.is_empty() {
        println!("  no matching workspaces");
        println!();
        return;
    }

    let now = OffsetDateTime::now_utc();
    let rows: Vec<(String, String, String)> = workspaces
        .iter()
        .map(|w| {
            let name = if show_prefix {
                format!("{}/{}", w.dir, w.slug)
            } else {
                w.slug.clone()
            };
            let desc = w.description.clone().unwrap_or_default();
            (name, desc, age_label(&w.created_at, now))
        })
        .collect();
    let name_width = rows
        .iter()
        .map(|(n, ..)| n.chars().count())
        .max()
        .unwrap_or(0);
    let desc_width = rows
        .iter()
        .map(|(_, d, _)| d.chars().count())
        .max()
        .unwrap_or(0);

    for (name, desc, age) in &rows {
        let name = format!("{name:<name_width$}");
        let desc = format!("{desc:<desc_width$}");
        println!(
            "  {}  {}  {}",
            name.if_supports_color(Stream::Stdout, |t| t.bold()),
            desc,
            age.if_supports_color(Stream::Stdout, |t| t.dimmed()),
        );
    }

    println!();
    let count = format!(
        "{} workspace{}",
        workspaces.len(),
        if workspaces.len() == 1 { "" } else { "s" }
    );
    println!(
        "  {}",
        count.if_supports_color(Stream::Stdout, |t| t.dimmed())
    );
}

/// Human age from an RFC3339 `created_at`, or empty if it cannot be parsed.
fn age_label(created_at: &str, now: OffsetDateTime) -> String {
    match OffsetDateTime::parse(created_at, &Rfc3339) {
        Ok(created) => humanize_age((now - created).whole_seconds()),
        Err(_) => String::new(),
    }
}
