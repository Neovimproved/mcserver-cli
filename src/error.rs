use std::{
    env::VarError,
    io,
    path::{self, PathBuf},
    result,
};

use kdl::KdlError;
use reqwest::header;
use thiserror::Error;

#[non_exhaustive]
#[derive(Debug, Error)]
pub enum ParseConfigError {
    #[error("Field {field} is an invalid type; expected type: {expected_type}")]
    InvalidType {
        field: &'static str,
        expected_type: &'static str,
    },

    #[error("Number {0} is out of bounds")]
    OutOfBounds(i128),

    #[error("Argument {arg} is absent, but {purpose}")]
    ExpectedArgument { arg: usize, purpose: &'static str },

    // #[error("Expected field {0}")]
    // ExpectedField(&'static str),
    #[error("Expected {0} to have children")]
    ExpectedChildren(&'static str),
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

    #[error("Invalid servers directory")]
    InvalidServersDirectory,

    #[error("Timestamp file ({0}) is invalid")]
    InvalidTimestampFile(String),

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

    #[error("Server {0} already exists")]
    ServerAlreadyExists(String),

    #[error("The machine's local time went backwards")]
    TimeWentBackwards,

    #[error("Server {0} was not found")]
    ServerNotFound(String),

    #[error(transparent)]
    StripPrefix(#[from] path::StripPrefixError),

    #[error("Template {0} already exists")]
    TemplateAlreadyExists(String),

    #[error("Template servers cannot be deployed")]
    TemplateDeployed,

    #[error("Template with the name {0} was not found")]
    TemplateNotFound(String),

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
