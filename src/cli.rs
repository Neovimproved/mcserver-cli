use std::{
    borrow::Cow, collections::HashSet, ffi::OsStr, os::unix::ffi::OsStrExt, path::{Path, PathBuf},
};

use clap::{ArgGroup, Args, Parser, Subcommand, ValueEnum, ValueHint, value_parser};
use clap_complete::{ArgValueCompleter, CompletionCandidate, Shell};

use crate::{
    config::{self, Config, get_directory},
    error::{Error, Result},
    server::{self, ServerId},
    session::get_alive_server_sessions,
};

#[derive(Parser)]
#[command(name = "mcserver", version, about)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

fn validate_server_path(
    path: &Path,
    session_whitelist: &HashSet<String>,
    config: &Config,
) -> Result<Option<String>> {
    let server_id = ServerId::from_path(path.to_path_buf());

    let session_name = server_id.try_as_session(config)?;

    Ok(if session_whitelist.contains(&session_name) {
        Some(server_id
            .try_as_string_relative(config)?
            .to_string())
    } else {
        None
    })
}

// Use a result purely because propagation is nice
fn try_get_servers(current: &OsStr) -> Result<Vec<CompletionCandidate>> {
    let mut servers = vec![];
    let (config, _) = config::load(get_directory()?.join(config::CONFIG_FILE_NAME))?;
    let server_sessions = get_alive_server_sessions()?;
    let current_str = str::from_utf8(current.as_bytes())?;

    server::for_each(
        |path| {
            let Ok(Some(server_name)) = validate_server_path(path, &server_sessions, &config) else {
                return;
            };

            if server_name.starts_with(current_str) {
                servers.push(CompletionCandidate::from(server_name));
            }
        },
        &config,
    )?;

    Ok(servers)
}

fn complete_inactive(current: &OsStr) -> Vec<CompletionCandidate> {
    try_get_servers(current).unwrap_or_else(|_| vec![])
}

fn complete_active(current: &OsStr) -> Vec<CompletionCandidate> {
    todo!()
}

#[non_exhaustive]
#[derive(Subcommand)]
pub enum Command {
    #[command(about = "Create an alias")]
    Alias {
        #[arg()]
        alias: Option<String>,

        #[arg(value_hint = ValueHint::DirPath, value_parser = value_parser!(ServerId))]
        server: Option<ServerId>,
    },

    #[command(visible_alias = "a", about = "Attach to a server session")]
    Attach {
        #[arg(value_hint = ValueHint::DirPath)]
        server: Option<ServerId>,
    },

    #[command(visible_alias = "cmp", about = "Generate completions for your shell")]
    Completions {
        #[arg(value_enum)]
        shell: Shell,
    },

    #[command(visible_alias = "cfg", about = "Get or edit the configuration")]
    Config {
        #[command(subcommand)]
        config_type: Option<ConfigType>,
    },

    #[command(
        subcommand = "delete-all-sessions",
        visible_alias = "da",
        about = "Safely delete all server dead server sessions"
    )]
    DeleteAllSessions {
        #[arg(short, long)]
        force: bool,
    },

    #[command(
        subcommand = "delete-session",
        visible_alias = "d",
        about = "Safely delete the session for a server (must be dead)"
    )]
    DeleteSession {
        #[arg(short, long)]
        force: bool,

        #[arg(value_hint = ValueHint::DirPath)]
        session: Option<ServerId>,
    },

    #[command(visible_alias = "dpl", about = "Deploy a server")]
    Deploy {
        #[arg(add = ArgValueCompleter::new(complete_inactive))]
        server: Option<ServerId>,
    },

    #[command(visible_alias = "exec", about = "Execute a command on a server")]
    Execute {
        #[arg(short, long)]
        server: Option<ServerId>,

        #[arg(trailing_var_arg = true)]
        commands: Vec<String>,
    },

    #[command(visible_alias = "ls", about = "List all, active or inactive servers")]
    List(ListingArguments),

    #[command(about = "Interact with a server, using the minecraft remote console")]
    Rcon {
        #[arg(add = ArgValueCompleter::new(complete_active))]
        server: Option<ServerId>,

        commands: Vec<String>,
    },

    #[command(about = "Create a new server")]
    New {
        #[clap(value_enum)]
        platform: Platform,

        #[arg(short, long)]
        name: Option<String>,

        #[arg(short, long)]
        version: Option<String>,
    },

    #[command(visible_alias = "reinst", about = "Reinstall the server binary",
        group(
                ArgGroup::new("source")
                    .args(&["git", "path", "from_crate"])
                    .required(true)
            )
    )]
    Reinstall {
        #[arg(short = 'c', long = "crate")]
        from_crate: bool,

        #[arg(short, long)]
        git: bool,

        #[arg(long, requires = "git")]
        commit: Option<String>,

        #[arg(short, long)]
        path: Option<PathBuf>,
    },

    #[command(visible_alias = "rm", about = "Remove a server")]
    Remove {
        #[arg(short, long)]
        force: bool,

        servers: Vec<ServerId>,
    },

    #[command(visible_alias = "rst", about = "Restart the current server")]
    Restart,

    #[command(about = "Stop a server")]
    Stop { server: Option<ServerId> },

    #[command(visible_alias = "tmpl", about = "Create or use a template server")]
    Template {
        #[command(subcommand)]
        action: TemplateCommands,
    },

    #[command(about = "List the servers in a tree")]
    Tree(ListingArguments),

    #[command(about = "Update a server's .jar file and reference")]
    Update {
        server: ServerId,

        platform: Platform,

        version: Option<String>,
    },
}

#[derive(Subcommand)]
pub enum ConfigType {
    Edit,
}

#[derive(Subcommand)]
pub enum DefaultCommands {
    Get,

    Set { server: String },
}

#[derive(Args, Debug)]
pub struct ListingArguments {
    #[arg(short, long, conflicts_with_all = ["inactive", "dead"])]
    pub active: bool,

    #[arg(short, long, conflicts_with = "active")]
    pub inactive: bool,

    #[arg(short, long, conflicts_with = "inactive")]
    pub dead: bool,
}

#[derive(Subcommand)]
pub enum TemplateCommands {
    New {
        server: ServerId,
    },

    From {
        template: ServerId,

        #[arg(short, long)]
        server: Option<ServerId>,
    },
}

#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, ValueEnum, Debug)]
pub enum Platform {
    Fabric,
    Forge,
    Neoforge,
    Paper,
    Purpur,
}
