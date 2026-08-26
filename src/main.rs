mod cli;
mod config;
mod error;
mod platforms;
mod server;
mod session;

use clap::{CommandFactory, Parser};
use clap_complete::generate;
use color_eyre::eyre::{Result, WrapErr};

use cli::*;

use crate::server::ServerOptionExt;

fn main() -> Result<()> {
    color_eyre::install()?;

    let args = Cli::parse();

    let config_dir = config::get_directory()?;
    let (config, mut document) = config::load_or_create(&config_dir)?;

    match args.command {
        Command::Alias { alias, server } => {
            let Some(alias) = alias else {
                println!("Aliases: ");
                for (alias, server) in config.aliases {
                    println!("{alias} -> {server}");
                }

                return Ok(());
            };

            if let Some(server) = server.map(|s| s.try_as_string(&config)).transpose()? {
                config::add_alias(&mut document, &alias, &server)?;
                config::write_document_to_config_file(&document, &config_dir)?;
                println!("Alias `{alias}` now references `{server}`");
            } else if let Some(server) = config.aliases.get(&alias) {
                println!("{alias} aliases {server}");
            } else {
                println!("{alias} does not alias anything");
            }
        }
        Command::Attach { server } => {
            session::attach(server.try_unwrap_or_fallback(&config)?, &config)
                .wrap_err("Failed to attach to session session")?
        }
        Command::Completions { shell } => {
            let cmd = Cli::command();
            generate(
                shell,
                &mut Cli::command(),
                cmd.get_name().to_string(),
                &mut std::io::stdout(),
            );
        }
        Command::Config { config_type } => {
            if config_type.is_some() {
                config::edit_config_file(&config_dir)?
            } else {
                println!("{config:#?}")
            }
        }
        Command::DeleteAllSessions { force } => if force {
            session::delete_all()
        } else {
            session::delete_all_confirmed()
        }
        .wrap_err("Failed to delete all sessions")?,
        Command::DeleteSession { session, force } => session::delete_server_session(
            session.try_unwrap_or_fallback(&config)?.as_session(),
            force,
        )
        .wrap_err("Failed to delete session")?,
        Command::Deploy { server } => {
            let server = server.try_unwrap_or_fallback(&config)?;
            let server_dir = server.try_as_absolute_path(&config)?;
            let metadata_dir = server_dir.join(server::METADATA_DIRECTORY_NAME);

            session::new_server(
                &server,
                &metadata_dir,
                Some(server::get_command(&server, &config)?),
            )?;
        }
        Command::Execute { server, commands } => {
            let session_name = server.try_unwrap_or_fallback(&config)?.as_session();
            for command in commands {
                session::write_line(&session_name, command)?;
            }
        }
        Command::List(listing_arguments) => {
            let servers = server::get_servers_list(listing_arguments, &config)
                .wrap_err("Failed to get servers")?;

            for server in &servers {
                println!("{server}");
            }

            println!("\n{} servers", servers.len());
        }
        Command::Rcon { server, commands } => {
            server::rcon(&server.try_unwrap_or_fallback(&config)?, commands, &config)
                .wrap_err("Failed to run rcon command")?
        }
        Command::New {
            platform,
            version,
            name,
        } => server::create_new(platform, version, name.as_deref(), &config)
            .wrap_err(format!("Failed to create {platform} server"))?,
        Command::Remove { servers, force } => if force {
            server::remove_servers(servers, &config)
        } else {
            server::remove_servers_with_confirmation(servers, &config)
        }
        .wrap_err("Failed to remove server")?,
        Command::Restart => server::restart(&config).wrap_err("Failed to restart server")?,
        Command::Stop { server } => {
            let server = server.try_unwrap_or_fallback(&config)?;
            server::rcon(&server, vec!["stop"], &config).wrap_err_with(|| {
                format!("Failed to stop server {}", server.try_as_string(&config)?)
            })?;
        }
        Command::Template { action } => match action {
            TemplateCommands::New { server } => server::new_template(&server, &config)
                .wrap_err_with(|| format!("Failed to create template with server {server}"))?,
            TemplateCommands::From { template, server } => {
                server::from_template(&template, server.as_deref(), &config)
                    .wrap_err_with(|| format!("Failed to use template {template}"))?
            }
        },
        Command::Tree(listing_arguments) => {
            let servers = server::get_servers_list(listing_arguments, &config)
                .wrap_err("Failed to get servers")?;

            println!(
                "{}",
                server::ServerTreeNode::try_from_flat_objects(servers, &config)
                    .wrap_err("Failed to convert servers into nodes")?
            );
        }
        Command::Reinstall {
            git,
            commit,
            path,
            from_crate,
        } => {
            if let Some(path) = path {
                server::reinstall_with_path(&path)
                    .wrap_err(format!("Failed to update package with {}", path.display()))?
            } else if git {
                server::reinstall_with_git(commit)
                    .wrap_err("Failed to update package with git repo")?
            } else if from_crate {
                server::reinstall_with_crate().wrap_err("Failed to update package with crate")?
            } else {
                unreachable!("Clap ensures git or a path is provided")
            }
        }
        Command::Update {
            server,
            platform,
            version,
        } => server::update_existing(server, platform, version, &config)
            .wrap_err("Failed to update server")?,
    };

    Ok(())
}
