use std::{
    collections::{HashMap, HashSet},
    ffi::OsStr,
    io::{self, Read, Write},
    path::{MAIN_SEPARATOR, Path},
    process::{Command, Stdio},
    thread,
    time::Duration,
};

use crate::{
    config::Config,
    error::{Error, Result},
    server::save_last_used_now,
    session,
};

pub const BASE_COMMAND: &str = "zellij";
pub const SUFFIX: &str = ".mcserver";

pub const TIMER: &str = "for i in {3..1}; do echo \"RESTARTING in $i seconds...\" && sleep 1; done";

pub fn path_str_to_session(server_path: impl AsRef<str>) -> String {
    format!("{}", server_path.as_ref().replace(MAIN_SEPARATOR, "."))
}

/// Get the session name of the server path
pub fn path_to_session(server_path: impl AsRef<Path>) -> Option<String> {
    Some(path_str_to_session(
        server_path.as_ref().to_str()?.replace(MAIN_SEPARATOR, "."),
    ))
}

fn get_server_sessions_raw_string() -> Result<Option<String>> {
    let output = Command::new(BASE_COMMAND).arg("list-sessions").output()?;

    match output.status.code() {
        Some(0) => Ok(Some(String::from_utf8_lossy(&output.stdout).into_owned())),
        Some(1) => Ok(None), // no sessions
        _ => Err(Error::CommandFailure {
            code: output.status.code(),
            stderr: Some(output.stderr),
        }),
    }
}

fn session_has_exited(session_line: impl AsRef<str>) -> bool {
    let session_line = session_line.as_ref();
    let bracket_pos = match session_line.rfind('(') {
        Some(pos) => pos,
        None => return false,
    };

    session_line[bracket_pos..].contains("EXITED") // if there is no "EXITED", still alive
}

fn session_is_alive(session_line: impl AsRef<str>) -> bool {
    !session_has_exited(session_line)
}

fn session_line_to_server(session_line: impl AsRef<str>) -> Option<String> {
    let session_line = session_line.as_ref();
    let session_name = &session_line[7..session_line.rfind("[Created")? - 4];

    session_name.strip_suffix(session::SUFFIX).map(String::from)
}

pub fn get_alive_server_sessions() -> Result<HashSet<String>> {
    let Some(server_sessions) = get_server_sessions_raw_string()? else {
        return Ok(HashSet::new());
    };

    Ok(server_sessions
        .lines()
        .filter(|sl| session_is_alive(sl))
        .filter_map(session_line_to_server)
        .collect())
}

pub fn get_dead_server_sessions() -> Result<HashSet<String>> {
    let Some(server_sessions) = get_server_sessions_raw_string()? else {
        return Ok(HashSet::new());
    };

    Ok(server_sessions
        .lines()
        .filter(|sl| session_has_exited(sl))
        .filter_map(session_line_to_server)
        .collect())
}

pub fn get_server_sessions_to_living() -> Result<HashMap<String, bool>> {
    let Some(server_sessions) = get_server_sessions_raw_string()? else {
        return Ok(HashMap::new());
    };

    Ok(server_sessions
        .lines()
        .map(|s| (s, session_is_alive(s)))
        .filter_map(|(session, living)| {
            session_line_to_server(session).map(|server| (server, living))
        })
        .collect())
}

pub fn attach(server: impl AsRef<str>, config: &Config) -> Result<()> {
    let server = server.as_ref();
    let mut child = Command::new(BASE_COMMAND)
        .arg("attach")
        .arg(path_str_to_session(server))
        .stderr(Stdio::piped())
        .spawn()?;

    let status = child.wait()?;

    if status.success() {
        save_last_used_now(server, config)
    } else {
        let mut buf = Vec::new();
        child
            .stderr
            .take()
            .ok_or(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "Failed to take stderr pipe",
            ))?
            .read_to_end(&mut buf)?;

        Err(Error::CommandFailure {
            code: status.code(),
            stderr: Some(buf),
        })
    }
}

pub fn new_session<S: AsRef<OsStr>, I: AsRef<OsStr>>(
    session: S,
    initial_command: Option<I>,
) -> Result<()> {
    Command::new(BASE_COMMAND)
        .arg("delete-session")
        .arg(&session)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()?;

    let mut command = Command::new(BASE_COMMAND);
    command.arg("--session").arg(&session);
    let mut child = command.spawn()?;

    thread::sleep(Duration::from_millis(300));

    if let Some(command) = initial_command {
        write_line(&session, command)?;
    }

    child.wait()?;

    Ok(())
}

pub fn new_server(
    server: &str,
    initial_command: Option<impl AsRef<OsStr>>,
    config: &Config,
) -> Result<()> {
    save_last_used_now(server, config)?;
    let session_name = path_str_to_session(server);
    new_session(session_name, initial_command)?;
    save_last_used_now(server, config)
}

pub fn delete_server_session(server: impl AsRef<str>, force: bool) -> Result<()> {
    let mut command = Command::new(BASE_COMMAND);
    command.arg("delete-session");
    command.arg(path_str_to_session(server));

    if force {
        command.arg("--force");
    }

    command.status()?;
    Ok(())
}

pub fn delete_all() -> Result<()> {
    for session in get_dead_server_sessions()? {
        delete_server_session(&session, false)?;
    }

    Ok(())
}

pub fn delete_all_confirmed() -> Result<()> {
    loop {
        print!("Delete all sessions? (y/n): ");
        io::stdout().flush()?;

        let mut confirmation = String::new();
        io::stdin().read_line(&mut confirmation)?;

        match confirmation.trim_end().to_lowercase().as_str() {
            "y" | "yes" => break delete_all()?,
            "n" | "no" => {
                println!("Operation canceled");
                break;
            }
            _ => {}
        };
    }

    Ok(())
}

fn session_write(
    session: impl AsRef<OsStr>,
    mode: &'static str,
    chars: impl AsRef<OsStr>,
) -> Result<()> {
    let status = Command::new(BASE_COMMAND)
        .arg("--session")
        .arg(session)
        .arg("action")
        .arg(mode)
        .arg(chars)
        .spawn()?
        .wait()?;

    if !status.success() {
        return Err(Error::CommandFailure {
            code: status.code(),
            stderr: None,
        });
    }

    Ok(())
}

pub fn write_chars(session: impl AsRef<OsStr>, chars: impl AsRef<OsStr>) -> Result<()> {
    session_write(session, "write-chars", chars)
}

pub fn write_line(session: impl AsRef<OsStr>, chars: impl AsRef<OsStr>) -> Result<()> {
    write_chars(&session, chars)?;
    session_write(&session, "write", "13")?; // 13 is for carriage return
    Ok(())
}

#[cfg(test)]
mod test {
    use std::{path::PathBuf, process::Command};

    use super::*;

    #[test]
    fn session_name() {
        assert_eq!(
            path_to_session(PathBuf::from("testing").join("test"))
                .expect("Expected session to be created"),
            format!("testing.test{SUFFIX}")
        );
    }

    #[test]
    fn wait_cmd() {
        let output = Command::new("bash")
            .arg("-c")
            .arg(TIMER)
            .output()
            .expect("Failed to run timer command");

        assert_eq!(
            output
                .status
                .code()
                .expect("Failed to get status code of timer"),
            0,
            "Timer returned code {}",
            output.status
        );
    }
}
