use std::path::PathBuf;
use std::process;

use clap::{CommandFactory, Parser, Subcommand};
use clap_complete::Shell;

mod commands;
mod config;
mod error;
mod store;
mod ui;
mod auth;
mod client;

use error::{CliError, CliResult};

// ---------------------------------------------------------------------------
// Global flags shared by every invocation
// ---------------------------------------------------------------------------

#[derive(Parser, Debug)]
#[command(
    name = "intr",
    version,
    about = "Intentry CLI — version-controlled prompt management",
    long_about = None,
    arg_required_else_help = true,
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,

    /// Output results as JSON
    #[arg(long, global = true)]
    json: bool,

    /// Suppress colors and ANSI escape sequences
    #[arg(long, global = true)]
    no_color: bool,

    /// Print extra debug information
    #[arg(long, short, global = true)]
    verbose: bool,

    /// Suppress all non-error output
    #[arg(long, short, global = true)]
    quiet: bool,

    /// Target space slug (overrides config default)
    #[arg(long, global = true)]
    space: Option<String>,

    /// Path to config file (default: ~/.intr/config.toml)
    #[arg(long, global = true)]
    config: Option<PathBuf>,
}

// ---------------------------------------------------------------------------
// Sub-command tree
// ---------------------------------------------------------------------------

#[derive(Subcommand, Debug)]
enum Commands {
    /// Initialise a new Intentry space in the current directory
    Init {
        /// Optional slug (defaults to directory name)
        slug: Option<String>,
    },

    /// Log in to an Intentry account
    Login,

    /// Log out of the current account
    Logout,

    /// Show the currently authenticated user
    Whoami,

    /// Read or write config values
    Config {
        #[command(subcommand)]
        cmd: ConfigCmd,
    },

    /// Manage remote spaces
    Space {
        #[command(subcommand)]
        cmd: SpaceCmd,
    },

    /// Scaffold a new .prompt file
    New {
        /// Prompt slug (kebab-case)
        slug: String,
        /// Tier (1, 2, or 3)
        #[arg(long, default_value = "2")]
        tier: u8,
        /// Create file without committing
        #[arg(long)]
        no_commit: bool,
    },

    /// List prompts in the current space
    #[command(alias = "ls")]
    List,

    /// Show the current version and metadata for a prompt
    Show {
        /// Prompt slug or ref (e.g. my-prompt, my-prompt@1.2.0)
        prompt: String,
        /// Show full content
        #[arg(long)]
        content: bool,
    },

    /// Open a prompt in $EDITOR
    Edit {
        /// Prompt slug
        prompt: String,
    },

    /// Stage and commit changed prompt files
    Commit {
        /// Commit message
        #[arg(short, long)]
        message: Option<String>,
        /// Semver bump (major|minor|patch)
        #[arg(long)]
        bump: Option<String>,
        /// Preview changes without writing
        #[arg(long)]
        dry_run: bool,
    },

    /// Show the commit history for a prompt
    Log {
        /// Prompt slug
        prompt: String,
    },

    /// Show the semantic diff between two .prompt files or versions
    Diff {
        /// Path to the old file (or version ref)
        from: Option<PathBuf>,
        /// Path to the new file (or version ref)
        to: Option<PathBuf>,
        /// Include unchanged sections
        #[arg(long)]
        all: bool,
    },

    /// Show working-copy status
    Status,

    /// Validate a .prompt file
    Parse {
        /// Path to the .prompt file
        file: PathBuf,
    },

    /// Execute a prompt against a model
    Run {
        /// Prompt ref (slug or slug@version)
        prompt_ref: String,
        /// Input string (or - to read from stdin)
        #[arg(short, long)]
        input: Option<String>,
        /// Model override
        #[arg(short, long)]
        model: Option<String>,
    },

    /// Run embedded evals for a prompt
    Eval {
        /// Prompt ref
        prompt_ref: String,
        /// Only run evals matching this name
        #[arg(long)]
        name: Option<String>,
    },

    /// Fork a prompt from another space
    Fork {
        /// Source ref (space/slug or space/slug@version)
        source: String,
    },

    /// Push local commits to the remote space
    Push,

    /// Pull remote commits into the local space
    Pull,

    /// Search the Intentry Commons for published prompts
    Search {
        /// Query string
        query: String,
        /// Max results
        #[arg(long, default_value = "20")]
        limit: u32,
    },

    /// Generate shell completion scripts
    Completions {
        /// Shell to generate completions for
        shell: Shell,
    },
}

#[derive(Subcommand, Debug)]
enum ConfigCmd {
    /// Get a config value
    Get { key: String },
    /// Set a config value
    Set { key: String, value: String },
    /// List all config values
    List,
}

#[derive(Subcommand, Debug)]
enum SpaceCmd {
    /// List available remote spaces
    List,
    /// Create a new remote space
    Create { slug: String },
    /// Show info about the current space
    Info,
    /// Switch to a different space
    Switch { slug: String },
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

#[tokio::main]
async fn main() {
    if let Err(e) = run().await {
        e.exit();
    }
}

async fn run() -> CliResult<()> {
    let cli = Cli::parse();

    // Apply global flags.
    if cli.no_color {
        // Safety: called once at startup before any threads are spawned by the
        // tokio runtime workers, and before any parallel reads of NO_COLOR.
        unsafe { std::env::set_var("NO_COLOR", "1") };
    }

    let json = cli.json;

    match cli.command {
        Commands::Init { slug } => {
            commands::init::run(slug.as_deref(), json)?;
        }
        Commands::Login => {
            commands::login::run(json).await?;
        }
        Commands::Logout => {
            commands::logout::run(json).await?;
        }
        Commands::Whoami => {
            commands::whoami::run(json).await?;
        }
        Commands::Config { cmd } => match cmd {
            ConfigCmd::Get { key } => {
                commands::config_cmd::get(&key, json)?;
            }
            ConfigCmd::Set { key, value } => {
                commands::config_cmd::set(&key, &value, json)?;
            }
            ConfigCmd::List => {
                commands::config_cmd::list(json)?;
            }
        },
        Commands::Space { cmd } => match cmd {
            SpaceCmd::List => {
                commands::space::list(json)?;
            }
            SpaceCmd::Create { slug } => {
                commands::space::create(&slug, json)?;
            }
            SpaceCmd::Info => {
                commands::space::info(json)?;
            }
            SpaceCmd::Switch { slug } => {
                commands::space::switch(&slug, json)?;
            }
        },
        Commands::New { slug, tier, no_commit } => {
            commands::new::run(&slug, tier, no_commit, json).await?;
        }
        Commands::List => {
            commands::ls::run(json).await?;
        }
        Commands::Show { prompt, content } => {
            commands::show::run(&prompt, content, json).await?;
        }
        Commands::Edit { prompt } => {
            commands::edit::run(&prompt, json).await?;
        }
        Commands::Commit { message, bump, dry_run } => {
            commands::commit::run(message.as_deref(), bump.as_deref(), dry_run, json).await?;
        }
        Commands::Log { prompt } => {
            commands::log_cmd::run(&prompt, json).await?;
        }
        Commands::Diff { from, to, all } => {
            commands::diff::run(from.as_deref(), to.as_deref(), all, json)?;
        }
        Commands::Status => {
            commands::status::run(json).await?;
        }
        Commands::Parse { file } => {
            commands::parse_cmd::run(&file, json)?;
        }
        Commands::Run { prompt_ref, input, model } => {
            commands::run::run(&prompt_ref, input.as_deref(), model.as_deref(), json)?;
        }
        Commands::Eval { prompt_ref, name } => {
            commands::eval::run(&prompt_ref, name.as_deref(), json)?;
        }
        Commands::Fork { source } => {
            commands::fork::run(&source, json)?;
        }
        Commands::Push => {
            commands::push::run(json).await?;
        }
        Commands::Pull => {
            commands::pull::run(json).await?;
        }
        Commands::Search { query, limit } => {
            commands::search::run(&query, limit, json).await?;
        }
        Commands::Completions { shell } => {
            let mut cmd = Cli::command();
            commands::completions::run(shell, &mut cmd)?;
        }
    }

    Ok(())
}
