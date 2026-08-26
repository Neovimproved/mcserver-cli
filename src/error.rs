use std::{
    env::VarError,
    io,
    path::{self, PathBuf},
    result,
    time::SystemTimeError,
};

use kdl::KdlError;
use reqwest::header;
use thiserror::Error;

use crate::config::{KdlValueType, NodeClass, NodeContext};

#[non_exhaustive]
#[derive(Debug, Error)]
pub enum ParseConfigError {
    #[error("Expected {0} to be type {1}")]
    InvalidType(NodeContext, KdlValueType),

    #[error("Number {0} is out of bounds")]
    OutOfBounds(i128),

    #[error("Value of node is absent (context: {0})")]
    ExpectedValue(NodeContext),

    // #[error("Expected field {0}")]
    // ExpectedField(&'static str),
    #[error("Expected {0} to have children")]
    ExpectedChildren(NodeClass),
}

#[derive(Debug, Error)]
pub enum InvalidServersDirectoryError {
    #[error("There are seemingly two servers or directories with the name {0}")]
    DuplicateServer(String),

    #[error("The server {} seems to be missing its parent", .0.display())]
    MissingParent(PathBuf),

    #[error("The server head of {} seems to be missing", .0.display())]
    MissingServerHead(PathBuf),
}

#[non_exhaustive]
#[derive(Debug, Error)]
pub enum Error {
    #[error(
        "Command failed with code {}{}",
        code.map(|c| c.to_string()).as_deref().unwrap_or("none"),
        stderr
            .as_ref()
            .map(|err| format!(": {}", String::from_utf8_lossy(err)))
            .as_deref()
            .unwrap_or("")
    )]
    CommandFailure {
        code: Option<i32>,
        stderr: Option<Vec<u8>>,
    },

    #[error(transparent)]
    ParseConfigError(#[from] ParseConfigError),

    #[error(transparent)]
    InvalidHeaderValue(#[from] header::InvalidHeaderValue),

    #[error("Invalid server string: {}", .0.display())]
    InvalidServerString(PathBuf),

    #[error(transparent)]
    InvalidServersDirectory(#[from] InvalidServersDirectoryError),

    #[error("Timestamp file ({}) is invalid", .0.display())]
    InvalidTimestampFile(PathBuf),

    #[error(transparent)]
    Io(#[from] io::Error),

    #[error(transparent)]
    KdlError(#[from] KdlError),

    #[error("Missing directory: {}", .0.display())]
    MissingDirectory(PathBuf),

    #[error("Missing file: {}", .0.display())]
    MissingFile(PathBuf),

    #[error("Config directory could not be resolved")]
    UnresolvedConfigDirectory,

    #[error("There is no default server")]
    NoDefaultServer,

    #[error("Platforms not found: {0}")]
    PlatformsNotFound(String),

    #[error("Rcon config is missing for server: {0}")]
    MissingRconConfig(String),

    #[error(transparent)]
    Reqwest(#[from] reqwest::Error),

    #[error(transparent)]
    ShellexpandLookup(#[from] shellexpand::LookupError<VarError>),

    #[error("A server already exists at {}", .0.display())]
    ServerAlreadyExists(PathBuf),

    #[error(transparent)]
    DurationMathFailure(#[from] SystemTimeError),

    #[error("Server {0} was not found")]
    ServerNotFound(String),

    #[error(transparent)]
    StripPrefix(#[from] path::StripPrefixError),

    #[error("Template servers cannot be deployed")]
    TemplateDeployed,

    #[error("Template at {0} was not found")]
    TemplateNotFound(PathBuf),

    #[error("Cannot create a template with a template")]
    TemplateUsedForTemplate,

    #[error(transparent)]
    ToStr(#[from] header::ToStrError),

    #[error(transparent)]
    UrlParse(#[from] url::ParseError),

    #[error(transparent)]
    VarError(#[from] VarError),
}

pub type Result<T> = result::Result<T, Error>;
