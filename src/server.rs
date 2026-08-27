use std::{
    borrow::Cow,
    cmp::Ordering,
    collections::{HashMap, HashSet, hash_map::Entry},
    convert::Infallible,
    env::{self, current_dir},
    ffi::OsStr,
    fmt::{self, Display, Formatter},
    fs::{self, File},
    io::{self, IsTerminal, Write},
    os::unix::fs::PermissionsExt,
    path::{MAIN_SEPARATOR, MAIN_SEPARATOR_STR, Path, PathBuf},
    process::Command,
    result,
    str::FromStr,
    time::{SystemTime, SystemTimeError, UNIX_EPOCH},
};

use reqwest::{
    blocking::{self, Response},
    header,
};
use url::Url;

use crate::{
    cli::{ListingArguments, Platform},
    config::Config,
    error::{Error, InvalidServersDirectoryError, Result},
    platforms::{self},
    session::{
        self, get_alive_server_sessions, get_dead_server_sessions, get_server_sessions_to_living,
        path_to_session,
    },
};

const REPO_URL: &str = env!("CARGO_PKG_REPOSITORY");
const TEMPLATE_SUFFIX: &str = ".template";

pub const METADATA_DIRECTORY_NAME: &str = ".mcserver";
const JAR_FILE_TXT_NAME: &str = "jar_file.txt";
pub const LAST_USED_FILE: &str = "last_used.timestamp";

const MULTIPLEXER_SESSION_NAME_VAR: &str = "ZELLIJ_SESSION_NAME";
const SERVER_NAME_VAR: &str = "SERVER_NAME";

#[derive(Clone, Debug)]
pub enum ServerId {
    /// Note: this variant is given relative to the servers directory
    Str(String),
    AbsolutePath(PathBuf),
}

impl FromStr for ServerId {
    type Err = Infallible;

    fn from_str(s: &str) -> result::Result<Self, Self::Err> {
        Ok(Self::Str(s.to_string()))
    }
}

fn current_absolute_server_dir(config: &Config) -> Result<PathBuf> {
    let mut server_path = env::current_dir()?;

    loop {
        if fs::exists(server_path.join(METADATA_DIRECTORY_NAME))? {
            break;
        }

        if !server_path.pop() {
            return Err(InvalidServersDirectoryError::MissingParent(server_path.to_owned()).into());
        }
    }

    if server_path.starts_with(config.servers_directory.expand()?) {
        Ok(server_path)
    } else {
        Err(Error::InvalidServerString(server_path))
    }
}

pub fn current_relative_server_dir(config: &Config) -> Result<PathBuf> {
    Ok(current_dir()?
        .strip_prefix(config.servers_directory.expand()?)?
        .to_path_buf())
}

impl ServerId {
    pub fn from_session(session: String) -> Option<Self> {
        session
            .strip_suffix(session::SUFFIX)
            .map(|s| s.replace('.', MAIN_SEPARATOR_STR))
            .map(Self::Str)
    }

    pub fn try_as_str_unresolved(&self) -> Result<&str> {
        match self {
            ServerId::Str(s) => Ok(s),
            ServerId::AbsolutePath(path_buf) => path_buf
                .to_str()
                .ok_or_else(|| Error::InvalidServerString(path_buf.clone())),
        }
    }

    pub fn from_var(var: String) -> Self {
        Self::Str(var)
    }

    pub fn try_as_session(&self) -> Result<String> {
        Ok(format!(
            "{}{}",
            self.try_as_str_unresolved()?.replace(MAIN_SEPARATOR, "."),
            session::SUFFIX
        ))
    }

    pub fn try_as_absolute_path(&self, config: &Config) -> Result<Cow<'_, Path>> {
        Ok(match self {
            ServerId::Str(server) => Cow::Owned(if server == "." {
                current_absolute_server_dir(config)?
            } else {
                config.servers_directory.expand()?.join(server)
            }),
            ServerId::AbsolutePath(path_buf) => Cow::Borrowed(path_buf),
        })
    }

    pub fn try_as_str_relative(&self, config: &Config) -> Result<Cow<'_, str>> {
        Ok(match self {
            ServerId::Str(server) => {
                if server == "." {
                    let dir = current_relative_server_dir(config)?
                        .into_os_string()
                        .into_string()
                        .map_err(|s| Error::InvalidServerString(PathBuf::from(s)))?;

                    Cow::Owned(dir)
                } else {
                    Cow::Borrowed(server)
                }
            }
            ServerId::AbsolutePath(path_buf) => Cow::Borrowed(
                path_buf
                    .strip_prefix(config.servers_directory.expand()?)?
                    .to_str()
                    .ok_or_else(|| Error::InvalidServerString(path_buf.to_owned()))?,
            ),
        })
    }

    /// The absolute path but as a string (lossless conversion used, erroring if non-utf-8 is present)
    pub fn try_as_str_absolute(&self, config: &Config) -> Result<Cow<'_, str>> {
        Ok(match self {
            ServerId::Str(server) => Cow::Owned(
                if server == "." {
                    current_absolute_server_dir(config)?
                } else {
                    config.servers_directory.expand()?.join(server)
                }
                .into_os_string()
                .into_string()
                .map_err(|s| Error::InvalidServerString(PathBuf::from(s)))?,
            ),
            ServerId::AbsolutePath(path_buf) => Cow::Borrowed(
                path_buf
                    .to_str()
                    .ok_or_else(|| Error::InvalidServerString(path_buf.to_owned()))?,
            ),
        })
    }
}

pub trait ServerOptionExt {
    fn try_unwrap_or_fallback(self, config: &Config) -> Result<ServerId>;
}

impl ServerOptionExt for Option<ServerId> {
    fn try_unwrap_or_fallback(self, config: &Config) -> Result<ServerId> {
        if let Some(server) = self {
            return Ok(server);
        }

        Ok(ServerId::Str(
            config
                .aliases
                .get("default")
                .ok_or(Error::NoDefaultServer)?
                .to_string(),
        ))
    }
}

#[derive(Debug, Eq, PartialEq)]
pub enum LastUsed {
    Never,
    Unknown,
    Time(String),
}

#[derive(Debug)]
struct Colorize<T: Display> {
    value: T,
    sequence: &'static str,
}

impl<T: Display> Display for Colorize<T> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        if io::stdout().is_terminal() {
            write!(f, "\x1b{}{}\x1b[0m", self.sequence, self.value)
        } else {
            write!(f, "{}", self.value)
        }
    }
}

impl Display for LastUsed {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            LastUsed::Never => write!(
                f,
                "(Last used {})",
                Colorize {
                    value: "never",
                    sequence: "[35;1m"
                }
            ),
            LastUsed::Unknown => write!(f, "(Last used unknown)"),
            LastUsed::Time(time) => write!(
                f,
                "(Last used {} ago)",
                Colorize {
                    value: time,
                    sequence: "[35;1m"
                }
            ),
        }
    }
}

#[derive(Debug, Eq, PartialEq)]
pub enum ServerState {
    Active,
    Dead,
}

impl Display for ServerState {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "({})",
            match self {
                ServerState::Active => Colorize {
                    value: "active",
                    sequence: "[32;1m",
                },
                ServerState::Dead => Colorize {
                    value: "dead",
                    sequence: "[31;1m",
                },
            }
        )
    }
}

#[derive(Debug, Eq, PartialEq)]
struct ServerTags {
    last_used: Option<LastUsed>,
    state: Option<ServerState>,
}

impl ServerTags {
    fn new() -> Self {
        Self {
            last_used: None,
            state: None,
        }
    }
}

impl Display for ServerTags {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        if let Some(state) = &self.state {
            write!(f, " {state}")?;
        }

        if let Some(last_used) = &self.last_used {
            write!(f, " {last_used}")?;
        }

        Ok(())
    }
}

#[derive(Debug, Eq)]
pub struct AbsoluteServerObject {
    path: PathBuf,
    tags: ServerTags,
}

impl PartialEq for AbsoluteServerObject {
    fn eq(&self, other: &Self) -> bool {
        self.path == other.path
    }
}

impl PartialOrd for AbsoluteServerObject {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for AbsoluteServerObject {
    fn cmp(&self, other: &Self) -> Ordering {
        self.path.cmp(&other.path)
    }
}

impl Display for AbsoluteServerObject {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.path.display())?;
        write!(f, "{}", self.tags)
    }
}

impl AbsoluteServerObject {
    pub fn new(path: PathBuf) -> Self {
        Self {
            path,
            tags: ServerTags::new(),
        }
    }

    fn set_last_used(&mut self, last_used: LastUsed) {
        self.tags.last_used = Some(last_used);
    }

    fn set_state(&mut self, state: ServerState) {
        self.tags.state = Some(state);
    }
}

#[derive(Debug)]
pub struct NamedServerObject {
    name: String,
    tags: ServerTags,
}

impl Display for NamedServerObject {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.name)?;
        write!(f, "{}", self.tags)
    }
}

impl TryFrom<AbsoluteServerObject> for NamedServerObject {
    type Error = ();

    fn try_from(value: AbsoluteServerObject) -> result::Result<Self, Self::Error> {
        Ok(Self {
            name: value
                .path
                .iter()
                .next_back()
                .ok_or(())?
                .to_string_lossy()
                .into_owned(),
            tags: value.tags,
        })
    }
}

#[derive(Debug)]
struct RelativeServerObject {
    relative_path: PathBuf,
    object: NamedServerObject,
}

#[derive(Debug)]
enum ServerUnwrapResult {
    Relative(String, RelativeServerObject),
    Finished(NamedServerObject),
}

impl RelativeServerObject {
    fn unwrap_layer(self) -> result::Result<ServerUnwrapResult, InvalidServersDirectoryError> {
        let mut components = self.relative_path.components();
        let removed = components.next();

        Ok(if let Some(removed) = removed {
            ServerUnwrapResult::Relative(
                removed.as_os_str().to_string_lossy().into_owned(),
                Self {
                    relative_path: components.collect(),
                    object: self.object,
                },
            )
        } else {
            ServerUnwrapResult::Finished(self.object)
        })
    }
}

#[derive(Debug)]
pub struct ServerDirectory {
    #[allow(unused)]
    name: String,
    children: HashMap<String, ServerTreeNode>,
    descendant_dir_count: usize,
    descendant_server_count: usize,
}

impl ServerDirectory {
    fn new(name: String) -> Self {
        Self {
            name,
            children: HashMap::new(),
            descendant_dir_count: 0,
            descendant_server_count: 0,
        }
    }

    fn insert(
        &mut self,
        obj: RelativeServerObject,
    ) -> result::Result<(), InvalidServersDirectoryError> {
        match obj.unwrap_layer()? {
            ServerUnwrapResult::Finished(named_server_object) => {
                self.add_object(named_server_object)?;
                self.descendant_server_count += 1;
            }
            ServerUnwrapResult::Relative(directory, relative_server_object) => {
                self.nest_object(directory, relative_server_object)?;
                self.descendant_dir_count += 1;
                self.descendant_server_count += 1;
            }
        };

        Ok(())
    }

    fn add_object(
        &mut self,
        obj: NamedServerObject,
    ) -> result::Result<(), InvalidServersDirectoryError> {
        match self.children.entry(obj.name.clone()) {
            Entry::Occupied(occupied) => {
                return Err(InvalidServersDirectoryError::DuplicateServer(
                    occupied.key().to_owned(),
                ));
            }
            Entry::Vacant(vacant_entry) => {
                vacant_entry.insert(ServerTreeNode::Child(obj));
            }
        }

        Ok(())
    }

    fn nest_object(
        &mut self,
        directory: String,
        obj: RelativeServerObject,
    ) -> result::Result<(), InvalidServersDirectoryError> {
        match self.children.entry(directory) {
            Entry::Occupied(mut occupied_entry) => match occupied_entry.get_mut() {
                ServerTreeNode::Child(child) => {
                    return Err(InvalidServersDirectoryError::DuplicateServer(
                        child.name.clone(),
                    ));
                }
                ServerTreeNode::Directory(server_parent) => {
                    server_parent.insert(obj)?;
                }
            },
            Entry::Vacant(vacant_entry) => {
                let name = vacant_entry.key().to_owned();

                let mut child = ServerDirectory::new(name);
                child.insert(obj)?;

                vacant_entry.insert(ServerTreeNode::Directory(child));
            }
        }

        Ok(())
    }
}

#[derive(Debug)]
#[allow(unused)]
pub enum ServerTreeNode {
    Child(NamedServerObject),
    Directory(ServerDirectory),
}

impl ServerTreeNode {
    pub fn try_from_flat_objects(
        objects: Vec<AbsoluteServerObject>,
        config: &Config,
    ) -> Result<Self> {
        let root_name = config.servers_directory.expand()?.to_string_lossy();

        let mut root = ServerDirectory::new(root_name.to_string());

        for obj in objects {
            let path = obj.path;
            let tags = obj.tags;

            let mut path_iter = path.iter();

            let name = path_iter
                .next_back()
                .ok_or_else(|| InvalidServersDirectoryError::MissingServerHead(path.clone()))?
                .to_string_lossy()
                .into_owned();

            root.insert(RelativeServerObject {
                relative_path: path_iter.collect(),
                object: NamedServerObject { name, tags },
            })?;
        }

        Ok(Self::Directory(root))
    }

    fn pretty_fmt(&self, f: &mut Formatter<'_>, base_indentation: &str) -> fmt::Result {
        match self {
            Self::Child(named_server_object) => {
                writeln!(f, "{named_server_object}")
            }
            Self::Directory(server_directory) => {
                // println!("For {}", server_directory.name);
                writeln!(
                    f,
                    "{}",
                    Colorize {
                        value: &server_directory.name,
                        sequence: "[34;1m"
                    },
                )?;

                let next_indentation = format!("{base_indentation}│   ");
                let last_idx = server_directory.children.iter().len().saturating_sub(1);

                for (idx, value) in server_directory.children.values().enumerate() {
                    write!(f, "{base_indentation}")?;

                    if idx == last_idx {
                        write!(f, "└── ")?;
                        value.pretty_fmt(f, &format!("{base_indentation}    "))?;
                    } else {
                        write!(f, "├── ")?;
                        value.pretty_fmt(f, &next_indentation)?;
                    }
                }

                Ok(())
            }
        }
    }

    #[allow(unused)]
    fn name(&self) -> &str {
        match self {
            Self::Child(named_server_object) => &named_server_object.name,
            Self::Directory(server_directory) => &server_directory.name,
        }
    }
}

impl Display for ServerTreeNode {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        // characters:
        // │
        // ├──
        // └──

        self.pretty_fmt(f, "")?;

        if let ServerTreeNode::Directory(dir) = self {
            write!(
                f,
                "\n{} {}, {} {}",
                dir.descendant_dir_count,
                if dir.descendant_dir_count == 1 {
                    "directory"
                } else {
                    "directories"
                },
                dir.descendant_server_count,
                if dir.descendant_server_count == 1 {
                    "server"
                } else {
                    "servers"
                }
            )
        } else {
            write!(f, "Fah")
        }
    }
}

pub fn copy_directory(src: impl AsRef<Path>, dst: impl AsRef<Path>) -> io::Result<()> {
    fs::create_dir_all(&dst)?;

    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let file_type = entry.file_type()?;

        if file_type.is_dir() {
            copy_directory(entry.path(), dst.as_ref().join(entry.file_name()))?;
        } else {
            fs::copy(entry.path(), dst.as_ref().join(entry.file_name()))?;
        }
    }

    Ok(())
}

pub fn remove_dir_with_retries(dir: impl AsRef<Path>) -> Result<()> {
    const ATTEMPTS: u8 = 10;

    for i in 1..=ATTEMPTS {
        if let Err(err) = fs::remove_dir_all(&dir) {
            if i == ATTEMPTS {
                return Err(Error::Io(err));
            }
        } else {
            return Ok(());
        }
    }

    unreachable!("Code returns before the for loop ends")
}

fn remove_server(server: ServerId, config: &Config) -> Result<()> {
    remove_dir_with_retries(server.try_as_absolute_path(config)?)
}

pub fn remove_servers(servers: Vec<ServerId>, config: &Config) -> Result<()> {
    let all_servers = get_all_hashed(config)?;

    for server in servers {
        let server_string = server.try_as_str_absolute(config)?;

        if !all_servers.contains(server_string.as_ref()) {
            return Err(Error::ServerNotFound(server_string.to_string()));
        }

        remove_server(server, config)?;
    }

    Ok(())
}

pub fn remove_servers_with_confirmation(servers: Vec<ServerId>, config: &Config) -> Result<()> {
    let all_servers = get_all_hashed(config)?;

    for server in servers {
        let server_string = server.try_as_str_absolute(config)?;

        if !all_servers.contains(server_string.as_ref()) {
            return Err(Error::ServerNotFound(server_string.to_string()));
        }

        if loop {
            print!("Enter `{server_string}` to delete the server or nothing to cancel operation: ");
            io::stdout().flush()?;

            let mut response = String::new();
            io::stdin().read_line(&mut response)?;

            if server_string == response.trim_end() {
                break true;
            } else if response.is_empty() {
                break false;
            }
        } {
            remove_server(server, config)?;
            println!("Server successfully removed");
        } else {
            println!("Operation canceled");
        }
    }

    Ok(())
}

pub fn set_last_used_metadata(last_used_file_path: &Path, timestamp: u64) -> Result<()> {
    let mut file = File::create(last_used_file_path)?;
    file.write_all(&timestamp.to_le_bytes())?;

    Ok(())
}

pub fn set_jar_file_metadata(metadata_dir: &Path, jar_file_name: &[u8]) -> Result<()> {
    let file_path = metadata_dir.join(JAR_FILE_TXT_NAME);

    if fs::exists(&file_path)? {
        let original_permissions = fs::metadata(&file_path)?.permissions();

        let mut new_permissions = original_permissions.clone();

        // writable by the owner
        new_permissions.set_mode(original_permissions.mode() | 0o200);

        fs::set_permissions(&file_path, new_permissions)?;

        fs::write(&file_path, jar_file_name)?;

        fs::set_permissions(&file_path, original_permissions)?;
    } else {
        fs::write(&file_path, jar_file_name)?;
    }

    Ok(())
}

pub fn set_default_metadata(metadata_dir: &Path, jar_file_name: &[u8]) -> Result<()> {
    fs::create_dir_all(metadata_dir)?;

    set_jar_file_metadata(metadata_dir, jar_file_name)?;
    set_last_used_metadata(&metadata_dir.join(LAST_USED_FILE), u64::MAX)?;

    Ok(())
}

fn copy_jar<S, F, J>(server_dir: S, file_name: F, mut jar: J) -> Result<()>
where
    S: AsRef<Path>,
    F: AsRef<Path>,
    J: io::Read,
{
    env::set_current_dir(server_dir)?;

    let mut jar_file = File::create(file_name)?;
    io::copy(&mut jar, &mut jar_file)?;

    Ok(())
}

pub fn get_jar(download_url: Url, platform: Platform) -> Result<(Response, String)> {
    println!("Downloading from {download_url}...");
    let response = blocking::get(download_url)?;

    let file_name = response
        .headers()
        .get(header::CONTENT_DISPOSITION)
        .map(|disposition| disposition.to_str())
        .transpose()?
        .and_then(|cd| cd.split("filename=\"").nth(1))
        .and_then(|slice| slice.split('"').next())
        .map(String::from)
        .unwrap_or_else(|| format!("{platform}.jar"));

    Ok((response, file_name))
}

pub fn create_new(
    platform: Platform,
    version: Option<String>,
    name: Option<&str>,
    config: &Config,
) -> Result<()> {
    let download_url = platforms::get(platform, version)?;

    let server_dir = match name {
        Some(name) => {
            let path = config.servers_directory.expand()?.join(name);

            if path.exists() {
                if path.join(METADATA_DIRECTORY_NAME).exists() {
                    eprint!("A server already exists at {}", path.display());
                } else if path.is_dir() {
                    eprintln!("A directory already exists at {}", path.display());
                } else {
                    eprintln!("Something already exists at {}", path.display());
                }

                return Ok(());
            }

            path
        }
        None => get_first_server_path(&format!("{platform}-server"), config)?,
    };

    fs::create_dir_all(&server_dir)?;
    let (jar, jar_file_name) = get_jar(download_url, platform)?;
    copy_jar(&server_dir, &jar_file_name, jar)?;

    set_default_metadata(
        &server_dir.join(METADATA_DIRECTORY_NAME),
        &jar_file_name.into_bytes(),
    )?;

    Ok(())
}

pub fn update_existing(
    server: ServerId,
    platform: Platform,
    version: Option<String>,
    config: &Config,
) -> Result<()> {
    let download_url = platforms::get(platform, version)?;

    let server_dir = server.try_as_absolute_path(config)?;

    let (jar, jar_file_name) = get_jar(download_url, platform)?;
    copy_jar(&server_dir, &jar_file_name, jar)?;

    set_jar_file_metadata(
        &server_dir.join(METADATA_DIRECTORY_NAME),
        &jar_file_name.into_bytes(),
    )?;

    Ok(())
}

pub fn get_unix_epoch_secs() -> result::Result<u64, SystemTimeError> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|dur| dur.as_secs())
}

pub fn get_last_used(server_dir: &Path) -> Result<LastUsed> {
    let timestamp_path = server_dir
        .join(METADATA_DIRECTORY_NAME)
        .join(LAST_USED_FILE);

    if !timestamp_path.exists() {
        return Ok(LastUsed::Unknown);
    }

    let data = fs::read(&timestamp_path)?;

    if data.len() != 8 {
        return Err(Error::InvalidTimestampFile(timestamp_path));
    }

    let bytes: [u8; 8] = data
        .try_into()
        .map_err(|_| Error::InvalidTimestampFile(timestamp_path))?;

    let timestamp = u64::from_le_bytes(bytes);

    if timestamp == u64::MAX {
        return Ok(LastUsed::Never);
    }

    let now_timestamp = get_unix_epoch_secs()?;

    // just in case ig
    let difference = now_timestamp.saturating_sub(timestamp);

    const SECS_MINUTE: u64 = 60;
    const SECS_HOUR: u64 = SECS_MINUTE * 60;
    const SECS_DAY: u64 = SECS_HOUR * 24;
    // no way am I doing months
    const SECS_YEAR: u64 = (SECS_DAY as f64 * 365.2425) as u64;

    let years = difference / SECS_YEAR;
    let years_remainder = difference % SECS_YEAR;

    let days = years_remainder / SECS_DAY;
    let days_remainder = years_remainder % SECS_DAY;

    let hours = days_remainder / SECS_HOUR;
    let hours_remainder = days_remainder % SECS_HOUR;

    let minutes = hours_remainder / SECS_MINUTE;
    let seconds = hours_remainder % SECS_MINUTE;

    Ok(LastUsed::Time(if years > 0 {
        format!("{years}y {days}d {hours}h {minutes}m {seconds}s")
    } else if days > 0 {
        format!("{days}d {hours}h {minutes}m {seconds}s")
    } else if hours > 0 {
        format!("{hours}h {minutes}m {seconds}s")
    } else if minutes > 0 {
        format!("{minutes}m {seconds}s")
    } else {
        format!("{seconds}s")
    }))
}

fn for_each_with_servers(
    f: &mut impl FnMut(&Path),
    parent_dir: impl AsRef<Path>,
    servers_dir: impl AsRef<Path>,
) -> Result<()> {
    for entry in fs::read_dir(parent_dir)? {
        let entry = entry?;

        let entry_path = entry.path();

        // TODO: fix the paths pls ok, thank you
        if fs::exists(entry_path.join(METADATA_DIRECTORY_NAME))? {
            f(entry_path.strip_prefix(&servers_dir)?);
        } else {
            for_each_with_servers(f, entry_path, servers_dir.as_ref())?;
        }
    }

    Ok(())
}

pub fn for_each(mut f: impl FnMut(&Path), config: &Config) -> Result<()> {
    let servers_dir = config.servers_directory.expand()?;

    if !servers_dir.exists() || !servers_dir.is_dir() {
        return Err(Error::MissingDirectory(servers_dir.to_path_buf()));
    }

    for_each_with_servers(&mut f, servers_dir, servers_dir)?;

    Ok(())
}

pub fn get_all_hashed(config: &Config) -> Result<HashSet<String>> {
    let mut servers = HashSet::<String>::new();

    for_each(
        |s| {
            servers.insert(s.to_string_lossy().into_owned());
        },
        config,
    )?;

    Ok(servers)
}

pub fn get_servers_list(
    listing_arguments: ListingArguments,
    config: &Config,
) -> Result<Vec<AbsoluteServerObject>> {
    let mut servers = vec![];

    for_each(
        |s| servers.push(AbsoluteServerObject::new(s.to_path_buf())),
        config,
    )?;

    let (active, inactive, dead) = (
        listing_arguments.active,
        listing_arguments.inactive,
        listing_arguments.dead,
    );

    if active {
        retain_active(&mut servers)?;
    } else if inactive {
        retain_and_tag_inactive(&mut servers)?;
        if dead {
            tag_dead(&mut servers)?;
        }
    } else if dead {
        retain_and_tag_dead(&mut servers)?;
    } else {
        fully_tag_servers(&mut servers)?;
    }

    servers.sort_unstable();

    Ok(servers)
}

fn get_server_jar_path_ensured(server_dir: impl AsRef<Path>) -> Result<PathBuf> {
    let server_dir = server_dir.as_ref();
    let jar_file_txt = server_dir
        .join(METADATA_DIRECTORY_NAME)
        .join(JAR_FILE_TXT_NAME);

    if !jar_file_txt.is_file() {
        return Err(Error::MissingFile(jar_file_txt));
    }

    let jar_file_path = server_dir.join(fs::read_to_string(jar_file_txt)?.trim_end());

    if !jar_file_path.is_file() {
        return Err(Error::MissingFile(jar_file_path));
    }

    Ok(jar_file_path)
}

pub fn get_command(server: &ServerId, config: &Config) -> Result<String> {
    let server_dir = server.try_as_absolute_path(config)?;

    if path_is_template(&server_dir) {
        return Err(Error::TemplateDeployed);
    }

    let server_name = server.try_as_str_relative(config)?;
    let server_dir_string = server.try_as_str_absolute(config)?;

    let server_jar_path = get_server_jar_path_ensured(&server_dir)?
        .into_os_string()
        .into_string()
        .map_err(|os_string| {
            let mut path_buf = PathBuf::from(os_string);
            path_buf.pop();
            Error::InvalidServerString(path_buf)
        })?;

    let java_args_string = config.default_java_args.join(" ");

    let nogui_option = if config.nogui { "nogui" } else { "" };

    let restart_timer = session::create_timer(config.restart_time);

    Ok(format!(
        "export {SERVER_NAME_VAR}={server_name} && {} action rename-tab Server && cd {server_dir_string} && while :; do java -jar {java_args_string} {server_jar_path} {nogui_option} && {restart_timer}; done",
        session::BASE_COMMAND,
    ))
}

pub fn restart(config: &Config) -> Result<()> {
    let server = match env::var(SERVER_NAME_VAR)
        .ok()
        .map(ServerId::from_var)
        .or_else(|| {
            env::var(MULTIPLEXER_SESSION_NAME_VAR)
                .ok()
                .and_then(ServerId::from_session)
        }) {
        Some(session) => session,
        None => {
            let cd = env::current_dir()?;
            let stripped = cd.strip_prefix(config.servers_directory.expand()?)?;
            ServerId::AbsolutePath(stripped.to_path_buf())
        }
    };

    let server_dir = server.try_as_absolute_path(config)?;

    set_last_used_metadata(
        &server_dir
            .join(METADATA_DIRECTORY_NAME)
            .join(LAST_USED_FILE),
        get_unix_epoch_secs()?,
    )?;

    session::write_line(server.try_as_session()?, get_command(&server, config)?)
}

pub fn path_is_template(server: &Path) -> bool {
    server.ends_with(TEMPLATE_SUFFIX)
}

pub fn new_template(server: &ServerId, config: &Config) -> Result<()> {
    let server_dir_binding = server.try_as_absolute_path(config)?;
    let server_dir = server_dir_binding.as_ref();

    if path_is_template(server_dir) {
        return Err(Error::TemplateUsedForTemplate);
    }

    let server_string = server.try_as_str_relative(config)?;

    println!("Creating template using server {server_string}...");

    if !server_dir.exists() {
        return Err(Error::ServerNotFound(server_string.to_string()));
    }

    let template_dir = server_dir.with_added_extension(TEMPLATE_SUFFIX);

    copy_directory(server_dir, template_dir)?;

    Ok(())
}

fn get_first_server_path(name: &str, config: &Config) -> Result<PathBuf> {
    let servers_dir = config.servers_directory.expand()?;
    let path = servers_dir.join(name);

    if !path.exists() {
        return Ok(path);
    }

    let mut number = 2;

    Ok(loop {
        let path = servers_dir.join(format!("{name}-{number}"));
        if !path.exists() {
            break path;
        }

        number += 1;
    })
}

pub fn from_template(template: &ServerId, server: Option<ServerId>, config: &Config) -> Result<()> {
    let template_path = template.try_as_absolute_path(config)?;

    let template_path = if !template_path.ends_with(TEMPLATE_SUFFIX) {
        Cow::Owned(template_path.as_ref().with_added_extension(TEMPLATE_SUFFIX))
    } else {
        template_path
    };

    println!("Creating server from {}", template_path.display());

    if !template_path.exists() {
        return Err(Error::TemplateNotFound(template_path.to_path_buf()));
    }

    let server_path = match server {
        Some(server) => {
            let path = server.try_as_absolute_path(config)?;

            if path.exists() {
                return Err(Error::ServerAlreadyExists(path.to_path_buf()));
            }

            path.into_owned()
        }
        None => get_first_server_path(template.try_as_str_relative(config)?.as_ref(), config)?,
    };

    copy_directory(template_path, server_path)?;

    Ok(())
}

pub fn reinstall_with_git(commit: Option<String>) -> io::Result<()> {
    Command::new("cargo")
        .arg("install")
        .arg("--git")
        .arg(if let Some(commit) = commit {
            format!("{REPO_URL}/commit/{commit}")
        } else {
            REPO_URL.to_string()
        })
        .arg("--force")
        .spawn()?
        .wait()?;

    Ok(())
}

pub fn reinstall_with_path(path: impl AsRef<OsStr>) -> io::Result<()> {
    Command::new("cargo")
        .arg("install")
        .arg("--path")
        .arg(path)
        .arg("--force")
        .spawn()?
        .wait()?;

    Ok(())
}

pub fn reinstall_with_crate() -> io::Result<()> {
    Command::new("cargo")
        .arg("install")
        .arg(env!("CARGO_PKG_NAME"))
        .spawn()?
        .wait()?;

    Ok(())
}

pub fn tag_dead(servers: &mut [AbsoluteServerObject]) -> Result<()> {
    let sessions = get_dead_server_sessions()?;

    servers.iter_mut().for_each(|server| {
        if let Some(session) = path_to_session(&server.path)
            && sessions.contains(&session)
        {
            server.set_state(ServerState::Dead);
        }
    });

    Ok(())
}

pub fn retain_active(servers: &mut Vec<AbsoluteServerObject>) -> Result<()> {
    let sessions = get_alive_server_sessions()?;
    servers.retain(|server| sessions.contains(server.path.to_string_lossy().as_ref()));
    Ok(())
}

fn add_last_used_tag(server: &mut AbsoluteServerObject) {
    server.set_last_used(get_last_used(&server.path).unwrap_or(LastUsed::Unknown));
}

pub fn retain_and_tag_inactive(servers: &mut Vec<AbsoluteServerObject>) -> Result<()> {
    let sessions = get_alive_server_sessions()?;

    servers.retain(|server| !sessions.contains(server.path.to_string_lossy().as_ref()));

    servers
        .iter_mut()
        .for_each(|server: &mut AbsoluteServerObject| add_last_used_tag(server));

    Ok(())
}

pub fn retain_and_tag_dead(servers: &mut Vec<AbsoluteServerObject>) -> Result<()> {
    let dead_sessions = get_dead_server_sessions()?;

    servers.retain(|server| {
        if let Some(session) = path_to_session(&server.path) {
            dead_sessions.contains(&session)
        } else {
            false
        }
    });

    servers.iter_mut().for_each(add_last_used_tag);
    Ok(())
}

pub fn fully_tag_servers(servers: &mut [AbsoluteServerObject]) -> Result<()> {
    let mapped_sessions = get_server_sessions_to_living()?;

    servers.iter_mut().try_for_each(|server| -> Result<()> {
        match mapped_sessions.get(
            &path_to_session(&server.path)
                .ok_or_else(|| Error::InvalidServerString(server.path.to_path_buf()))?,
        ) {
            Some(true) => server.set_state(ServerState::Active),
            Some(false) => {
                add_last_used_tag(server);
                server.set_state(ServerState::Dead);
            }
            None => add_last_used_tag(server),
        };

        Ok(())
    })?;

    Ok(())
}

pub fn rcon<T: AsRef<OsStr>>(
    server_string: &str,
    commands: impl AsRef<[T]>,
    config: &Config,
) -> Result<()> {
    let rcon_config = &config.rcon;

    for (k, v) in rcon_config.iter() {
        println!("{:?}: {v:?}", k.chars().next());
    }

    let server_rcon_config = rcon_config
        .get(server_string)
        .or_else(|| rcon_config.get(&format!("\"{server_string}\"")))
        .ok_or_else(|| Error::MissingRconConfig(server_string.to_string()))?;

    let mut command = Command::new("mcrcon");

    if let Some(server_address) = &server_rcon_config.server_address {
        command.arg("-H");
        command.arg(server_address);
    }

    if let Some(port) = &server_rcon_config.port {
        command.arg("-P");
        command.arg(port.to_string());
    }

    if let Some(password) = &server_rcon_config.password {
        command.arg("-p");
        command.arg(password);
    }

    for arg in commands.as_ref() {
        command.arg(arg);
    }

    let status = command.status()?;

    if status.success() {
        Ok(())
    } else {
        Err(Error::CommandFailure {
            code: status.code(),
            stderr: None,
        })
    }
}

#[cfg(test)]
mod test {

    // #[test]
    // fn idk() {
    //     todo!();
    // }
}
