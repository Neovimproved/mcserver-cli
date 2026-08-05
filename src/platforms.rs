use std::{
    fmt::{self, Display, Formatter},
    sync::OnceLock,
};

use reqwest::{
    self,
    blocking::{self, Client},
    header::{HeaderMap, HeaderValue, USER_AGENT},
};
use serde::Deserialize;
use url::Url;

use crate::{
    cli::Platform,
    config::Config,
    error::{Error, Result},
};

static CLIENT: OnceLock<Client> = OnceLock::new();

const FABRIC_BASE_API_URL: &str = "https://meta.fabricmc.net/v2/versions";

const PAPER_BASE_API_URL: &str = "https://api.papermc.io/v2/projects/paper";
const PAPER_BASE_DOWNLOAD_URL: &str = "https://fill-data.papermc.io/v1/objects";

const PURPUR_BASE_API_URL: &str = "https://api.purpurmc.org/v2/purpur";

fn get_client(config: &Config) -> Result<&'static Client> {
    if let Some(client) = CLIENT.get() {
        return Ok(client);
    }

    let mut headers = HeaderMap::new();
    headers.insert(USER_AGENT, HeaderValue::from_str(&config.contact)?);

    let client = Client::builder().default_headers(headers).build()?;

    Ok(CLIENT.get_or_init(|| client))
}

#[derive(Debug, Deserialize)]
struct FabricEntry {
    version: String,
    stable: bool,
}

#[derive(Debug, Deserialize)]
struct FabricVersions {
    game: Vec<FabricEntry>,
    loader: Vec<FabricEntry>,
    installer: Vec<FabricEntry>,
}

fn first_stable(entries: Vec<FabricEntry>) -> Option<FabricEntry> {
    entries.into_iter().find(|entry| entry.stable)
}

fn get_fabric(game_version: Option<String>) -> Result<String> {
    let versions: FabricVersions = blocking::get(FABRIC_BASE_API_URL)?.json()?;

    let game_version = game_version.map_or_else(
        || {
            first_stable(versions.game)
                .map(|e| e.version)
                .ok_or_else(|| Error::PlatformsNotFound("stable game version".to_string()))
        },
        Ok,
    )?;
    let loader_version = first_stable(versions.loader)
        .ok_or_else(|| Error::PlatformsNotFound("stable loader".to_string()))?
        .version;
    let installer_version = first_stable(versions.installer)
        .ok_or_else(|| Error::PlatformsNotFound("stable installer".to_string()))?
        .version;

    Ok(format!(
        "{FABRIC_BASE_API_URL}/loader/{game_version}/{loader_version}/{installer_version}/server/jar",
    ))
}

#[derive(Debug, Deserialize)]
struct PaperProjectInfo {
    versions: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct PaperBuildsInfo {
    builds: Vec<Build>,
}

#[derive(Debug, Deserialize)]
struct Build {
    downloads: PaperDownloads,
}

#[derive(Debug, Deserialize)]
struct PaperDownloads {
    application: PaperApplication,
}

#[derive(Debug, Deserialize)]
struct PaperApplication {
    name: String,
    sha256: String,
}

fn get_paper(version: Option<String>, config: &Config) -> Result<String> {
    let client = get_client(config)?;

    let version = version.map_or_else(
        || {
            let project_info: PaperProjectInfo = client.get(PAPER_BASE_API_URL).send()?.json()?;
            let mut versions = project_info.versions;
            Ok::<_, Error>(versions.pop().unwrap())
        },
        Ok,
    )?;

    let builds: Vec<Build> = client
        .get(format!("{PAPER_BASE_API_URL}/versions/{version}/builds"))
        .send()?
        .json::<PaperBuildsInfo>()?
        .builds;
    let application = &builds[builds.len() - 1].downloads.application;

    let download_url = format!(
        "{PAPER_BASE_DOWNLOAD_URL}/{}/{}",
        application.sha256, application.name
    );

    Ok(download_url)
}

#[derive(Debug, Deserialize)]
struct PurpurProjectInfo {
    metadata: PurpurMetadata,
}

#[derive(Debug, Deserialize)]
struct PurpurMetadata {
    current: String,
}

#[derive(Debug, Deserialize)]
struct PurpurVersionInfo {
    builds: PurpurBuilds,
}

#[derive(Debug, Deserialize)]
struct PurpurBuilds {
    latest: String,
}

fn get_current_purpur_version() -> Result<String> {
    let project_info: PurpurProjectInfo = blocking::get(PURPUR_BASE_API_URL)?.json()?;
    Ok(project_info.metadata.current)
}

fn get_purpur(version: Option<String>) -> Result<String> {
    let version = version.map_or_else(get_current_purpur_version, Ok)?;

    let version_url = format!("{PURPUR_BASE_API_URL}/{version}");
    let version_info: PurpurVersionInfo = blocking::get(&version_url)?.json()?;

    let latest = version_info.builds.latest;
    println!("Creating purpur server (v{version}, build {latest})");

    let download_url = format!("{version_url}/{latest}/download");
    Ok(download_url)
}

impl Display for Platform {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Fabric => write!(f, "fabric"),
            Self::Forge => write!(f, "forge"),
            Self::Neoforge => write!(f, "neoforge"),
            Self::Paper => write!(f, "paper"),
            Self::Purpur => write!(f, "purpur"),
        }
    }
}

pub fn get(platform: Platform, version: Option<String>, config: &Config) -> Result<Url> {
    // set version to none if the it is "latest" so that it defaults to the latest one
    let version = version.filter(|v| v != "latest");

    let download_url = match platform {
        Platform::Fabric => get_fabric(version)?,
        Platform::Forge => todo!(),
        Platform::Neoforge => todo!(),
        Platform::Paper => get_paper(version, config)?,
        Platform::Purpur => get_purpur(version)?,
    };

    Ok(Url::parse(&download_url)?)
}
