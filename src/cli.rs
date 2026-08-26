use std::path::PathBuf;

use clap::{ArgGroup, Args, Parser, Subcommand, ValueEnum, ValueHint, value_parser};
use clap_complete::Shell;

use crate::server::Server;

#[derive(Parser)]
#[command(name = "mcserver", version, about)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[non_exhaustive]
#[derive(Subcommand)]
pub enum Command {
    #[command(about = "Create an alias")]
    Alias {
        #[arg()]
        alias: Option<String>,

        #[arg(value_hint = ValueHint::DirPath, value_parser = value_parser!(Server))]
        server: Option<Server>,
    },

    #[command(visible_alias = "a", about = "Attach to a server session")]
    Attach {
        #[arg(value_hint = ValueHint::DirPath)]
        server: Option<Server>,
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
        session: Option<Server>,
    },

    #[command(visible_alias = "dpl", about = "Deploy a server")]
    Deploy {
        #[arg(value_hint = ValueHint::DirPath)]
        server: Option<Server>,
    },

    #[command(visible_alias = "exec", about = "Execute a command on a server")]
    Execute {
        #[arg(short, long)]
        server: Option<Server>,

        #[arg(trailing_var_arg = true)]
        commands: Vec<String>,
    },

    #[command(visible_alias = "ls", about = "List all, active or inactive servers")]
    List(ListingArguments),

    #[command(about = "Interact with a server, using the minecraft remote console")]
    Rcon {
        server: Option<Server>,

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

        servers: Vec<Server>,
    },

    #[command(visible_alias = "rst", about = "Restart the current server")]
    Restart,

    #[command(about = "Stop a server")]
    Stop { server: Option<Server> },

    #[command(visible_alias = "tmpl", about = "Create or use a template server")]
    Template {
        #[command(subcommand)]
        action: TemplateCommands,
    },

    #[command(about = "List the servers in a tree")]
    Tree(ListingArguments),

    #[command(about = "Update a server's .jar file and reference")]
    Update {
        server: Server,

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
        server: Server,
    },

    From {
        template: Server,

        #[arg(short, long)]
        server: Option<Server>,
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
