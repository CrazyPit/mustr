use std::error::Error;
use std::io::{IsTerminal, Write};
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use owo_colors::{OwoColorize, Stream};
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

use mustr::agent::{self, AgentKind};
use mustr::context::{self, Context};
use mustr::dir;
use mustr::mount;
use mustr::project;
use mustr::render::humanize_age;
use mustr::slug::slugify;
use mustr::source::{self, SourceKind};
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
    /// Print a workspace's path, for `cd "$(mustr path tb-123)"`
    Path {
        /// Address `[dir/]slug`
        address: String,
        /// Project slug (defaults to the selected project)
        #[arg(short = 'p', long)]
        project: Option<String>,
    },
    /// Manage a project's sources (external repos and dirs)
    #[command(alias = "src")]
    Source {
        /// Project slug to act on (defaults to the selected project)
        #[arg(short = 'p', long, global = true)]
        project: Option<String>,
        #[command(subcommand)]
        command: SourceCommand,
    },
    /// Open coding agents in a workspace
    #[command(alias = "a")]
    Agent {
        /// Project slug (defaults to the cwd project)
        #[arg(short = 'p', long, global = true)]
        project: Option<String>,
        /// Workspace address `[dir/]slug` (defaults to the cwd workspace)
        #[arg(short = 'w', long, global = true)]
        workspace: Option<String>,
        #[command(subcommand)]
        command: AgentCommand,
    },
}

#[derive(Subcommand)]
enum AgentCommand {
    /// Open an agent (resumes its session, or starts a fresh one)
    Open {
        /// Agent kind (currently only `claude`)
        kind: String,
        /// Agent slug — pass to run more than one of a kind (default: main)
        slug: Option<String>,
    },
}

#[derive(Subcommand)]
enum WsSourceCommand {
    /// Materialize a project source into src/ (worktree for git, symlink for dir)
    #[command(alias = "new")]
    Add {
        /// Source slug to materialize (omit with --all)
        slug: Option<String>,
        /// Materialize every project source
        #[arg(short = 'a', long)]
        all: bool,
        /// Worktree branch (default: the workspace slug)
        #[arg(long)]
        branch: Option<String>,
    },
    /// List materialized sources
    #[command(alias = "ls")]
    List,
    /// Remove a materialized source from src/
    #[command(alias = "remove")]
    Rm {
        /// Source slug
        slug: String,
        /// Skip confirmation and force-remove a dirty worktree
        #[arg(
            short = 'f',
            long = "force",
            visible_alias = "yes",
            visible_short_alias = 'y'
        )]
        force: bool,
    },
}

#[derive(Subcommand)]
enum SourceCommand {
    /// Register a git repository
    AddGit {
        /// Path to a local git repository
        path: String,
        /// Slug (defaults to the slugified repo folder name)
        slug: Option<String>,
        /// Base branch (auto-detected if omitted)
        #[arg(long = "base-branch")]
        base_branch: Option<String>,
    },
    /// Register a plain directory
    AddDir {
        /// Path to a directory
        path: String,
        /// Slug (defaults to the slugified folder name)
        slug: Option<String>,
    },
    /// Remove a source (entry only; the real repo/dir is untouched)
    #[command(alias = "remove")]
    Rm {
        /// Source slug
        slug: String,
    },
    /// Rename a source
    #[command(alias = "mv")]
    Rename {
        /// Current slug
        slug: String,
        /// New slug
        new_slug: String,
    },
    /// List sources
    #[command(alias = "ls")]
    List,
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
    /// Remove a workspace (soft-delete to trash; --permanent to delete)
    #[command(alias = "remove")]
    Rm {
        /// Address `[dir/]slug`
        address: String,
        /// Delete permanently instead of moving to trash
        #[arg(long)]
        permanent: bool,
        /// Skip the confirmation prompt
        #[arg(
            short = 'f',
            long = "force",
            visible_alias = "yes",
            visible_short_alias = 'y'
        )]
        force: bool,
    },
    /// Permanently empty the trash dir
    Purge {
        /// Skip the confirmation prompt
        #[arg(
            short = 'f',
            long = "force",
            visible_alias = "yes",
            visible_short_alias = 'y'
        )]
        force: bool,
    },
    /// Manage the sources materialized in this workspace's src/
    #[command(alias = "src")]
    Source {
        /// Workspace address `[dir/]slug` (defaults to the cwd workspace)
        #[arg(short = 'w', long, global = true)]
        workspace: Option<String>,
        #[command(subcommand)]
        command: WsSourceCommand,
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
        #[arg(
            short = 'f',
            long = "force",
            visible_alias = "yes",
            visible_short_alias = 'y'
        )]
        force: bool,
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
        #[arg(
            short = 'f',
            long = "force",
            visible_alias = "yes",
            visible_short_alias = 'y'
        )]
        force: bool,
    },
    /// Rename a project (renames its folder)
    #[command(alias = "mv")]
    Rename {
        /// Current slug
        slug: String,
        /// New slug (spaces and punctuation are normalized)
        new_slug: String,
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
    let ctx = current_context(&store);
    match cli.command {
        Command::Project { command } => run_project(&store, &ctx, command),
        Command::Dir { project, command } => run_dir(&store, &ctx, project, command),
        Command::Workspace { project, command } => run_workspace(&store, &ctx, project, command),
        Command::Path { address, project } => {
            let project = resolve_project(&ctx, project)?;
            let (dir, slug) = workspace::parse_address(&address);
            let path = workspace::path(&store, &project, &dir, &slug)?;
            println!("{}", path.display());
            Ok(())
        }
        Command::Source { project, command } => run_source(&store, &ctx, project, command),
        Command::Agent {
            project,
            workspace,
            command,
        } => run_agent(&store, &ctx, project, workspace, command),
    }
}

fn run_agent(
    store: &Store,
    ctx: &Context,
    project: Option<String>,
    workspace: Option<String>,
    command: AgentCommand,
) -> Result<(), Box<dyn Error>> {
    let project = resolve_project(ctx, project)?;
    let (dir, ws) = resolve_workspace(ctx, &project, workspace)?;
    match command {
        AgentCommand::Open { kind, slug } => {
            let kind = match kind.as_str() {
                "claude" => AgentKind::Claude,
                other => {
                    return Err(format!("unknown agent '{other}' (only 'claude' for now)").into())
                }
            };
            let slug = slug.unwrap_or_else(|| "main".to_string());
            let agent = agent::resolve(store, &project, &dir, &ws, kind, &slug)?;
            let cwd = store.workspace_path(&project, &dir, &ws);

            match agent::plan(&agent, &cwd, &claude_home(), process_alive)? {
                agent::OpenPlan::AlreadyRunning { pid } => Err(format!(
                    "claude '{slug}' is already running (pid {pid}) — focus that session"
                )
                .into()),
                agent::OpenPlan::Launch { args, cwd, resume } => {
                    eprintln!(
                        "{} claude '{slug}' (session {})",
                        if resume { "resuming" } else { "starting" },
                        agent.session_id
                    );
                    let err = std::process::Command::new("claude")
                        .args(&args)
                        .current_dir(&cwd)
                        .exec();
                    Err(format!("failed to exec claude: {err}").into())
                }
            }
        }
    }
}

/// Claude's config dir: `CLAUDE_CONFIG_DIR` if set, else `~/.claude`.
fn claude_home() -> PathBuf {
    if let Some(dir) = std::env::var_os("CLAUDE_CONFIG_DIR") {
        return PathBuf::from(dir);
    }
    let home = std::env::var_os("HOME").expect("HOME environment variable is not set");
    PathBuf::from(home).join(".claude")
}

/// Whether `pid` is a live process, via `kill -0`.
fn process_alive(pid: u32) -> bool {
    std::process::Command::new("kill")
        .args(["-0", &pid.to_string()])
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// The context derived from the current working directory.
fn current_context(store: &Store) -> Context {
    std::env::current_dir()
        .map(|cwd| context::context_from(store, &cwd))
        .unwrap_or_default()
}

/// Resolves the project to act on: the `--project` flag, else the project the
/// cwd is in. Errors when neither is available.
fn resolve_project(ctx: &Context, flag: Option<String>) -> Result<String, Box<dyn Error>> {
    flag.or_else(|| ctx.project.clone())
        .ok_or_else(|| "not inside a project — pass --project <slug>".into())
}

fn run_source(
    store: &Store,
    ctx: &Context,
    project: Option<String>,
    command: SourceCommand,
) -> Result<(), Box<dyn Error>> {
    let project = resolve_project(ctx, project)?;
    match command {
        SourceCommand::AddGit {
            path,
            slug,
            base_branch,
        } => {
            let s = source::add_git(
                store,
                &project,
                &path,
                slug.as_deref(),
                base_branch.as_deref(),
            )?;
            let branch = s.base_branch.as_deref().unwrap_or("?");
            println!(
                "Added git source {} ({branch}) -> {}",
                s.slug,
                s.path.display()
            );
        }
        SourceCommand::AddDir { path, slug } => {
            let s = source::add_dir(store, &project, &path, slug.as_deref())?;
            println!("Added dir source {} -> {}", s.slug, s.path.display());
        }
        SourceCommand::Rm { slug } => {
            source::remove(store, &project, &slug)?;
            println!("Removed source {slug}");
        }
        SourceCommand::Rename { slug, new_slug } => {
            let s = source::rename(store, &project, &slug, &new_slug)?;
            println!("Renamed source {slug} → {}", s.slug);
        }
        SourceCommand::List => print_sources(store, &project)?,
    }
    Ok(())
}

fn run_workspace(
    store: &Store,
    ctx: &Context,
    project: Option<String>,
    command: WorkspaceCommand,
) -> Result<(), Box<dyn Error>> {
    let project = resolve_project(ctx, project)?;
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
            permanent,
            force,
        } => {
            let (dir, slug) = workspace::parse_address(&address);
            let permanent = permanent || dir == "trash";
            if permanent
                && !force
                && !confirm_removal(&format!("workspace '{dir}/{slug}' permanently"))?
            {
                println!("Aborted.");
                return Ok(());
            }
            match workspace::remove(store, &project, &dir, &slug, permanent)? {
                Removal::Deleted => println!("Deleted workspace {dir}/{slug}"),
                Removal::Trashed { slug: final_slug } if final_slug == slug => {
                    println!("Moved {dir}/{slug} to trash");
                }
                Removal::Trashed { slug: final_slug } => {
                    println!("Moved {dir}/{slug} to trash as {final_slug} (name was taken)");
                }
            }
        }
        WorkspaceCommand::Purge { force } => {
            if !force && !confirm_removal("all workspaces in trash")? {
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
            print_workspaces(
                &scope,
                &workspaces,
                dir.is_none(),
                current_workspace(ctx, &project),
            );
        }
        WorkspaceCommand::Grep { query, all } => {
            let workspaces = workspace::grep(store, &project, &query, all)?;
            print_workspaces(
                &format!("{project} · grep: {query}"),
                &workspaces,
                true,
                current_workspace(ctx, &project),
            );
        }
        WorkspaceCommand::Source { workspace, command } => {
            run_ws_source(store, ctx, &project, workspace, command)?;
        }
    }
    Ok(())
}

fn run_ws_source(
    store: &Store,
    ctx: &Context,
    project: &str,
    workspace: Option<String>,
    command: WsSourceCommand,
) -> Result<(), Box<dyn Error>> {
    let (dir, ws) = resolve_workspace(ctx, project, workspace)?;
    let project = project.to_string();
    match command {
        WsSourceCommand::Add { slug, all, branch } => {
            if all {
                let added = mount::add_all(store, &project, &dir, &ws)?;
                println!(
                    "Materialized {} source{}",
                    added.len(),
                    if added.len() == 1 { "" } else { "s" }
                );
            } else if let Some(slug) = slug {
                let m = mount::add(store, &project, &dir, &ws, &slug, branch.as_deref())?;
                match m.kind {
                    mount::MountKind::Worktree { branch } => {
                        println!("Added worktree {} on {branch}", m.slug)
                    }
                    mount::MountKind::Link { .. } => println!("Linked {}", m.slug),
                }
            } else {
                return Err("pass a source slug or --all".into());
            }
        }
        WsSourceCommand::List => print_mounts(store, &project, &dir, &ws)?,
        WsSourceCommand::Rm { slug, force } => {
            if !force && !confirm_removal(&format!("source '{slug}' from {ws}"))? {
                println!("Aborted.");
                return Ok(());
            }
            mount::remove(store, &project, &dir, &ws, &slug, force)?;
            println!("Removed source {slug} from {ws}");
        }
    }
    Ok(())
}

/// Resolves (project, dir, workspace) for a `w src` command: `-w` address or the
/// cwd workspace; `-p` or cwd project.
fn resolve_workspace(
    ctx: &Context,
    project: &str,
    workspace: Option<String>,
) -> Result<(String, String), Box<dyn Error>> {
    if let Some(addr) = workspace {
        return Ok(workspace::parse_address(&addr));
    }
    if ctx.project.as_deref() == Some(project) {
        if let (Some(dir), Some(ws)) = (&ctx.dir, &ctx.workspace) {
            return Ok((dir.clone(), ws.clone()));
        }
    }
    Err("not inside a workspace — pass --workspace [dir/]slug".into())
}

fn print_mounts(
    store: &Store,
    project: &str,
    dir: &str,
    workspace: &str,
) -> Result<(), Box<dyn Error>> {
    let mounts = mount::list(store, project, dir, workspace)?;

    println!();
    let header = format!("sources · {project}/{dir}/{workspace}");
    println!(
        "  {}",
        header.if_supports_color(Stream::Stdout, |t| t.dimmed())
    );
    println!();

    if mounts.is_empty() {
        println!("  no sources — add one with `mustr w src add <slug>`");
        println!();
        return Ok(());
    }

    let slug_width = mounts
        .iter()
        .map(|m| m.slug.chars().count())
        .max()
        .unwrap_or(0);
    for m in &mounts {
        let (kind, detail) = match &m.kind {
            mount::MountKind::Worktree { branch } => ("worktree", branch.clone()),
            mount::MountKind::Link { target } => ("link", target.display().to_string()),
        };
        let slug = format!("{:<slug_width$}", m.slug);
        println!(
            "  {}  {}  {}",
            slug.if_supports_color(Stream::Stdout, |t| t.bold()),
            format_args!("{kind:<8}").if_supports_color(Stream::Stdout, |t| t.dimmed()),
            detail.if_supports_color(Stream::Stdout, |t| t.dimmed()),
        );
    }

    println!();
    let count = format!(
        "{} source{}",
        mounts.len(),
        if mounts.len() == 1 { "" } else { "s" }
    );
    println!(
        "  {}",
        count.if_supports_color(Stream::Stdout, |t| t.dimmed())
    );
    Ok(())
}

/// The (dir, slug) of the workspace the cwd is in, if it is in `project`.
fn current_workspace(ctx: &Context, project: &str) -> Option<(String, String)> {
    if ctx.project.as_deref() != Some(project) {
        return None;
    }
    match (&ctx.dir, &ctx.workspace) {
        (Some(dir), Some(slug)) => Some((dir.clone(), slug.clone())),
        _ => None,
    }
}

fn run_dir(
    store: &Store,
    ctx: &Context,
    project: Option<String>,
    command: DirCommand,
) -> Result<(), Box<dyn Error>> {
    let project = resolve_project(ctx, project)?;
    match command {
        DirCommand::Add { slug } => {
            let created = dir::add(store, &project, &slug)?;
            println!("Created dir {} in {project}", created.slug);
        }
        DirCommand::Rm { slug, force } => {
            if !force && !confirm_removal(&format!("dir '{slug}' in '{project}'"))? {
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

fn run_project(
    store: &Store,
    ctx: &Context,
    command: ProjectCommand,
) -> Result<(), Box<dyn Error>> {
    match command {
        ProjectCommand::Add { slug } => {
            let project = project::add(store, &slug)?;
            println!("Created project {}", project.slug);
        }
        ProjectCommand::Rm { slug, force } => {
            if !force && !confirm_removal(&format!("project '{slug}'"))? {
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
        ProjectCommand::List => print_list(store, ctx)?,
    }
    Ok(())
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

fn print_list(store: &Store, ctx: &Context) -> Result<(), Box<dyn Error>> {
    let projects = project::list(store)?;
    let current = ctx.project.as_deref();

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
        let is_current = current == Some(project.slug.as_str());
        let marker = if is_current { "★" } else { " " };
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

fn print_workspaces(
    scope: &str,
    workspaces: &[Workspace],
    show_prefix: bool,
    current: Option<(String, String)>,
) {
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
    let name_of = |w: &Workspace| {
        if show_prefix {
            format!("{}/{}", w.dir, w.slug)
        } else {
            w.slug.clone()
        }
    };
    let name_width = workspaces
        .iter()
        .map(|w| name_of(w).chars().count())
        .max()
        .unwrap_or(0);
    let desc_width = workspaces
        .iter()
        .map(|w| w.description.as_deref().unwrap_or("").chars().count())
        .max()
        .unwrap_or(0);

    for w in workspaces {
        let is_current = current.as_ref() == Some(&(w.dir.clone(), w.slug.clone()));
        let marker = if is_current { "★" } else { " " };
        let name = format!("{:<name_width$}", name_of(w));
        let desc = format!("{:<desc_width$}", w.description.as_deref().unwrap_or(""));
        println!(
            "  {} {}  {}  {}",
            marker.if_supports_color(Stream::Stdout, |t| t.yellow()),
            name.if_supports_color(Stream::Stdout, |t| t.bold()),
            desc,
            age_label(&w.created_at, now).if_supports_color(Stream::Stdout, |t| t.dimmed()),
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

fn print_sources(store: &Store, project: &str) -> Result<(), Box<dyn Error>> {
    let sources = source::list(store, project)?;

    println!();
    let header = format!("sources · {project}");
    println!(
        "  {}",
        header.if_supports_color(Stream::Stdout, |t| t.dimmed())
    );
    println!();

    if sources.is_empty() {
        println!("  no sources — add one with `mustr source add-git <path>`");
        println!();
        return Ok(());
    }

    let slug_width = sources
        .iter()
        .map(|s| s.slug.chars().count())
        .max()
        .unwrap_or(0);
    let branch_width = sources
        .iter()
        .map(|s| s.base_branch.as_deref().unwrap_or("").chars().count())
        .max()
        .unwrap_or(0);

    for s in &sources {
        let kind = match s.kind {
            SourceKind::Git => "git",
            SourceKind::Dir => "dir",
        };
        let slug = format!("{:<slug_width$}", s.slug);
        let branch = format!("{:<branch_width$}", s.base_branch.as_deref().unwrap_or(""));
        println!(
            "  {}  {}  {}  {}",
            slug.if_supports_color(Stream::Stdout, |t| t.bold()),
            kind.if_supports_color(Stream::Stdout, |t| t.dimmed()),
            branch.if_supports_color(Stream::Stdout, |t| t.dimmed()),
            s.path
                .display()
                .if_supports_color(Stream::Stdout, |t| t.dimmed()),
        );
    }

    println!();
    let count = format!(
        "{} source{}",
        sources.len(),
        if sources.len() == 1 { "" } else { "s" }
    );
    println!(
        "  {}",
        count.if_supports_color(Stream::Stdout, |t| t.dimmed())
    );
    Ok(())
}

/// Human age from an RFC3339 `created_at`, or empty if it cannot be parsed.
fn age_label(created_at: &str, now: OffsetDateTime) -> String {
    match OffsetDateTime::parse(created_at, &Rfc3339) {
        Ok(created) => humanize_age((now - created).whole_seconds()),
        Err(_) => String::new(),
    }
}
