mod cli;
mod error;
#[allow(dead_code)]
mod server;
#[allow(dead_code)]
mod session;

pub mod config;
pub mod platforms;

pub use error::{Error, Result};
