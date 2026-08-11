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
use config::handle_server_arg;

fn main() -> Result<()> {
    color_eyre::install()?;

    let args = Cli::parse();

    let config_dir = config::get_directory()?;
    let (config, mut document) = config::load_or_create(&config_dir)?;

    match args.command {
        Command::Alias { alias, server } => {
            let Some(alias) = alias else {
                for (alias, server) in config.aliases {
                    println!("{alias} -> {server}");
                }

                return Ok(());
            };

            if let Some(server) = server {
                config::add_alias(&mut document, alias, server)?;
                config::write_document_to_config_file(&document, &config_dir)?
            } else if let Some(server) = config.aliases.get(&alias) {
                println!("{alias} aliases {server}");
            } else {
                println!("{alias} does not alias anything");
            }
        }
        Command::Attach { server } => session::attach(handle_server_arg(server, &config)?, &config)
            .wrap_err("Failed to attach to session session")?,
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
        Command::DeleteSession { session, force } => {
            session::delete_server_session(handle_server_arg(session, &config)?, force)
                .wrap_err("Failed to delete session")?
        }
        Command::Deploy { server } => {
            let server = handle_server_arg(server, &config)?;
            session::new_server(
                &server,
                Some(server::get_command(&server, &config)?),
                &config,
            )?;
        }
        Command::Execute { server, commands } => {
            let session_name = session::get_name(handle_server_arg(server, &config)?);
            for command in commands {
                session::write_line(&session_name, command)?;
            }
        }
        Command::List {
            active,
            inactive,
            dead,
            flat,
        } => {
            let mut servers = vec![];
            server::for_each(
                |s| servers.push(server::AbsoluteServerObject::new(s.to_path_buf())),
                &config,
            )
            .wrap_err("Failed to get servers")?;

            if active {
                server::retain_active(&mut servers).wrap_err("Failed to retain active servers")?;
            } else if inactive {
                server::retain_and_tag_inactive(&mut servers, &config)
                    .wrap_err("Failed to retain inactive servers")?;
                if dead {
                    server::tag_dead(&mut servers).wrap_err("Failed to tag dead servers")?;
                }
            } else if dead {
                server::retain_and_tag_dead(&mut servers, &config)
                    .wrap_err("Failed to retain dead servers")?;
            } else {
                server::fully_tag_servers(&mut servers, &config)
                    .wrap_err("Failed to tag active servers")?;
            }

            if flat {
                for server in servers {
                    println!("{server}");
                }
            } else {
                println!(
                    "{}",
                    server::ServerTreeNode::try_from_flat_objects(servers)?
                )
            }
        }
        Command::Rcon { server, commands } => {
            server::rcon(handle_server_arg(server, &config)?, commands, &config)
                .wrap_err("Failed to run rcon command")?
        }
        Command::New {
            platform,
            version,
            name,
        } => server::create_new(platform, version, name, &config)
            .wrap_err(format!("Failed to create {platform} server"))?,
        Command::Remove { servers, force } => if force {
            server::remove_servers(servers, &config)
        } else {
            server::remove_servers_with_confirmation(servers, &config)
        }
        .wrap_err("Failed to remove server")?,
        Command::Restart => server::restart(&config).wrap_err("Failed to restart server")?,
        Command::Stop { server } => {
            let server = handle_server_arg(server, &config)?;
            server::rcon(&server, vec!["stop"], &config)
                .wrap_err_with(|| format!("Failed to stop server {}", server))?;
        }
        Command::Template { action } => match action {
            TemplateCommands::New { server } => server::new_template(&server, &config)
                .wrap_err_with(|| format!("Failed to create template with server {server}"))?,
            TemplateCommands::From { template, server } => {
                server::from_template(&template, server.as_deref(), &config)
                    .wrap_err_with(|| format!("Failed to use template {template}"))?
            }
        },
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
