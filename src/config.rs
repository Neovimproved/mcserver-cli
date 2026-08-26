use std::{
    cell::UnsafeCell,
    collections::HashMap,
    env::{self, VarError},
    ffi::OsStr,
    fmt::{self, Debug, Display, Formatter},
    fs, io,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    result,
};

use kdl::{KdlDocument, KdlIdentifier, KdlNode, KdlValue};
use shellexpand::LookupError;

use crate::error::{Error, ParseConfigError, Result};

const DEFAULT_CONFIG: &str = include_str!(concat!(env!("OUT_DIR"), "/generated_config.kdl"));

const CONFIG_DIRECTORY_NAME: &str = "mcserver";
const CONFIG_FILE_NAME: &str = "config.kdl";

pub struct Password(pub String);

impl AsRef<OsStr> for Password {
    fn as_ref(&self) -> &OsStr {
        OsStr::new(&self.0)
    }
}

impl Debug for Password {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "(hidden)")
    }
}

#[derive(Debug)]
pub struct RconConfig {
    pub server_address: Option<String>,
    pub port: Option<u16>,
    pub password: Option<Password>,
}

pub struct ServersDirectory {
    raw_string: String,
    expanded: UnsafeCell<Option<PathBuf>>,
}

impl Debug for ServersDirectory {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self.expand() {
            Ok(path) => write!(f, "{}", path.display()),
            Err(_) => write!(f, "[failed to expand servers directory]"),
        }
    }
}

impl From<String> for ServersDirectory {
    fn from(value: String) -> Self {
        Self {
            raw_string: value,
            expanded: UnsafeCell::new(None),
        }
    }
}

impl From<&str> for ServersDirectory {
    fn from(value: &str) -> Self {
        Self::from(value.to_string())
    }
}

impl ServersDirectory {
    pub fn expand(&self) -> result::Result<&Path, LookupError<VarError>> {
        unsafe {
            if let Some(expanded) = &*(self.expanded.get()) {
                return Ok(expanded);
            }

            let expanded_path = PathBuf::from(shellexpand::full(&self.raw_string)?.into_owned());
            Ok((*self.expanded.get()).insert(expanded_path))
        }
    }
}

#[derive(Debug)]
pub struct Config {
    pub default_java_args: Vec<String>,
    pub nogui: bool,
    pub servers_directory: ServersDirectory,
    pub aliases: HashMap<String, String>,
    pub restart_time: usize,
    pub rcon: HashMap<String, RconConfig>,
}

#[derive(Debug)]
pub enum NodeClass {
    Aliases,
    Rcon,
}

impl Display for NodeClass {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}",
            match self {
                Self::Aliases => "aliases",
                Self::Rcon => "rcon",
            }
        )
    }
}

#[derive(Debug)]
pub struct NodeContext {
    node_name: KdlIdentifier,
    parent_class: Option<NodeClass>,
}

impl Display for NodeContext {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        if let Some(node_class) = &self.parent_class {
            write!(f, "{} of {}", self.node_name, node_class)
        } else {
            write!(f, "{}", self.node_name)
        }
    }
}

impl NodeContext {
    fn new(node_name: KdlIdentifier) -> Self {
        Self {
            node_name,
            parent_class: None,
        }
    }

    fn with_parent_class(node_name: KdlIdentifier, parent_class: NodeClass) -> Self {
        Self {
            node_name,
            parent_class: Some(parent_class),
        }
    }
}

#[derive(Debug)]
pub enum KdlValueType {
    String,
    Integer,
    Float,
    Bool,
}

impl Display for KdlValueType {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}",
            match self {
                Self::String => "string",
                Self::Integer => "integer",
                Self::Float => "float",
                Self::Bool => "bool",
            }
        )
    }
}

#[allow(unused)]
trait KdlNodeExt {
    fn get_value(&self) -> result::Result<&KdlValue, ParseConfigError>;

    fn get_string_value(&self) -> result::Result<&str, ParseConfigError>;

    fn get_integer_value(&self) -> result::Result<i128, ParseConfigError>;

    fn get_float_value(&self) -> result::Result<f64, ParseConfigError>;

    fn get_bool_value(&self) -> result::Result<bool, ParseConfigError>;

    fn get_nested_value(
        &self,
        parent_class: NodeClass,
    ) -> result::Result<&KdlValue, ParseConfigError>;

    fn get_nested_string_value(
        &self,
        parent_class: NodeClass,
    ) -> result::Result<&str, ParseConfigError>;

    fn get_nested_integer_value(
        &self,
        parent_class: NodeClass,
    ) -> result::Result<i128, ParseConfigError>;

    fn get_nested_float_value(
        &self,
        parent_class: NodeClass,
    ) -> result::Result<f64, ParseConfigError>;

    fn get_nested_bool_value(
        &self,
        parent_class: NodeClass,
    ) -> result::Result<bool, ParseConfigError>;
}

impl KdlNodeExt for KdlNode {
    fn get_value(&self) -> result::Result<&KdlValue, ParseConfigError> {
        self.get(0).ok_or_else(|| {
            ParseConfigError::ExpectedValue(NodeContext::new(self.name().to_owned()))
        })
    }

    fn get_string_value(&self) -> result::Result<&str, ParseConfigError> {
        self.get_value()?.as_string().ok_or_else(|| {
            ParseConfigError::InvalidType(
                NodeContext::new(self.name().to_owned()),
                KdlValueType::String,
            )
        })
    }

    fn get_integer_value(&self) -> result::Result<i128, ParseConfigError> {
        self.get_value()?.as_integer().ok_or_else(|| {
            ParseConfigError::InvalidType(
                NodeContext::new(self.name().to_owned()),
                KdlValueType::Integer,
            )
        })
    }

    fn get_float_value(&self) -> result::Result<f64, ParseConfigError> {
        self.get_value()?.as_float().ok_or_else(|| {
            ParseConfigError::InvalidType(
                NodeContext::new(self.name().to_owned()),
                KdlValueType::Float,
            )
        })
    }

    fn get_bool_value(&self) -> result::Result<bool, ParseConfigError> {
        self.get_value()?.as_bool().ok_or_else(|| {
            ParseConfigError::InvalidType(
                NodeContext::new(self.name().to_owned()),
                KdlValueType::Bool,
            )
        })
    }

    fn get_nested_value(
        &self,
        parent_class: NodeClass,
    ) -> result::Result<&KdlValue, ParseConfigError> {
        self.get(0).ok_or_else(|| {
            ParseConfigError::ExpectedValue(NodeContext::with_parent_class(
                self.name().to_owned(),
                parent_class,
            ))
        })
    }

    fn get_nested_string_value(
        &self,
        parent_class: NodeClass,
    ) -> result::Result<&str, ParseConfigError> {
        self.get_nested_value(parent_class)?
            .as_string()
            .ok_or_else(|| {
                ParseConfigError::InvalidType(
                    NodeContext::new(self.name().to_owned()),
                    KdlValueType::String,
                )
            })
    }

    fn get_nested_integer_value(
        &self,
        parent_class: NodeClass,
    ) -> result::Result<i128, ParseConfigError> {
        self.get_nested_value(parent_class)?
            .as_integer()
            .ok_or_else(|| {
                ParseConfigError::InvalidType(
                    NodeContext::new(self.name().to_owned()),
                    KdlValueType::Integer,
                )
            })
    }

    fn get_nested_float_value(
        &self,
        parent_class: NodeClass,
    ) -> result::Result<f64, ParseConfigError> {
        self.get_nested_value(parent_class)?
            .as_float()
            .ok_or_else(|| {
                ParseConfigError::InvalidType(
                    NodeContext::new(self.name().to_owned()),
                    KdlValueType::Float,
                )
            })
    }

    fn get_nested_bool_value(
        &self,
        parent_class: NodeClass,
    ) -> result::Result<bool, ParseConfigError> {
        self.get_nested_value(parent_class)?
            .as_bool()
            .ok_or_else(|| {
                ParseConfigError::InvalidType(
                    NodeContext::new(self.name().to_owned()),
                    KdlValueType::Bool,
                )
            })
    }
}

pub fn get_directory() -> Result<PathBuf> {
    let config_dir = dirs::config_dir()
        .ok_or(Error::UnresolvedConfigDirectory)?
        .join(CONFIG_DIRECTORY_NAME);

    Ok(config_dir)
}

pub fn edit_config_file(config_directory: &Path) -> Result<()> {
    Command::new(env::var("EDITOR")?)
        .arg(config_directory.join(CONFIG_FILE_NAME))
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .output()?;

    Ok(())
}

fn parse_alias(node: &KdlNode) -> result::Result<(String, String), ParseConfigError> {
    let reference = node
        .get_nested_string_value(NodeClass::Aliases)?
        .to_string();

    Ok((node.name().value().to_string(), reference))
}

fn transform_number<T, E>(
    node: &KdlNode,
    f: impl Fn(i128) -> result::Result<T, E>,
    parent_class: Option<NodeClass>,
) -> result::Result<T, ParseConfigError> {
    let integer = if let Some(parent_class) = parent_class {
        node.get_nested_integer_value(parent_class)
    } else {
        node.get_integer_value()
    }?;

    f(integer).map_err(|_| ParseConfigError::OutOfBounds(integer))
}

fn parse_rcon_config(node: &KdlNode) -> result::Result<(String, RconConfig), ParseConfigError> {
    let children = node
        .children()
        .ok_or(ParseConfigError::ExpectedChildren(NodeClass::Rcon))?;

    let rcon_config = RconConfig {
        server_address: children
            .get("server_address")
            .map(|node| node.get_nested_string_value(NodeClass::Rcon))
            .transpose()?
            .map(String::from),

        port: children
            .get("port")
            .map(|node| transform_number(node, u16::try_from, Some(NodeClass::Rcon)))
            .transpose()?,

        password: children
            .get("password")
            .map(|node| node.get_nested_string_value(NodeClass::Rcon))
            .transpose()?
            .map(|value| Password(value.to_string())),
    };

    Ok((node.name().value().to_string(), rcon_config))
}

fn parse_config(document: &KdlDocument) -> Result<Config> {
    let default_java_args: Vec<String> = document
        .get("default_java_args")
        .map(|node| {
            node.iter_children()
                .map(|child_node| format!("\"{}\"", child_node.name().value()))
                .collect()
        })
        .unwrap_or_default();

    let nogui = document
        .get("nogui")
        .map(|node| node.get_bool_value())
        .transpose()?
        .unwrap_or(true);

    let servers_directory = ServersDirectory::from(
        document
            .get("servers_directory")
            .map(KdlNodeExt::get_string_value)
            .transpose()?
            .unwrap_or("~/Servers"),
    );

    let aliases: HashMap<String, String> = document
        .get("aliases")
        .map(|node| node.iter_children().map(parse_alias).collect())
        .transpose()?
        .unwrap_or_default();

    let rcon: HashMap<String, RconConfig> = document
        .get("rcon")
        .map(|node| node.iter_children().map(parse_rcon_config).collect())
        .transpose()?
        .unwrap_or_default();

    Ok(Config {
        default_java_args,
        nogui,
        servers_directory,
        aliases,
        rcon,
        restart_time: 3,
    })
}

pub fn write_document_to_config_file(
    document: &KdlDocument,
    config_directory: &Path,
) -> io::Result<()> {
    let config_file_path = config_directory.join(CONFIG_FILE_NAME);

    fs::write(config_file_path, document.to_string())
}

pub fn load_or_create(config_directory: &Path) -> Result<(Config, KdlDocument)> {
    let config_file_path = config_directory.join(CONFIG_FILE_NAME);

    let document = match fs::read_to_string(&config_file_path) {
        Ok(config_str) => KdlDocument::parse(&config_str),
        Err(err) => {
            if err.kind() == io::ErrorKind::NotFound {
                fs::create_dir_all(config_directory)?;
                fs::write(&config_file_path, DEFAULT_CONFIG)?;
                println!("Created default configuration");
                KdlDocument::parse(DEFAULT_CONFIG)
            } else {
                return Err(Error::Io(err));
            }
        }
    }?;

    Ok((parse_config(&document)?, document))
}

fn quote_protect_kdl_str(s: &str) -> String {
    let mut buf = '"'.to_string();

    for ch in s.chars() {
        match ch {
            '"' => buf.push_str("\\\""),
            '\n' => buf.push_str("\\n"),
            '\r' => buf.push_str("\\r"),
            x => buf.push(x),
        }
    }

    buf.push('"');

    buf
}

fn check_char_needs_quotes(ch: char) -> bool {
    matches!(
        ch,
        '(' | ')' | '{' | '}' | '[' | ']' | '/' | '\\' | '"' | '#' | ';' | '=' | '\n' | '\r'
    )
}

fn transform_to_kdl_string(raw_str: &str) -> String {
    let mut chars = raw_str.chars();

    let Some(first) = chars.next() else {
        return "".to_string();
    };

    if check_char_needs_quotes(first) || first.is_ascii_digit() || matches!(first, '.' | '-') {
        return quote_protect_kdl_str(raw_str);
    }

    for ch in chars {
        if check_char_needs_quotes(ch) {
            return quote_protect_kdl_str(raw_str);
        }
    }

    raw_str.to_string()
}

pub fn add_alias(document: &mut KdlDocument, alias: &str, server: &str) -> Result<()> {
    if alias.len() > server.len() && alias != "default" {
        println!("Definitely practical");
    }

    let kdl_alias = transform_to_kdl_string(alias);
    let kdl_server = transform_to_kdl_string(server);

    if let Some(aliases_node) = document.get_mut("aliases") {
        let children = aliases_node
            .children_mut()
            .as_mut()
            .ok_or(ParseConfigError::ExpectedChildren(NodeClass::Aliases))?;

        // Remove existing alias(es) with the same name and remember the index of the first found
        // copy of the alias if it is found
        let mut idx = 0;
        let mut alias_found = false;

        children.nodes_mut().retain(|c| {
            let retain_item = c.name().value() != alias;

            if retain_item {
                if !alias_found {
                    idx += 1;
                }
            } else {
                alias_found = true;
            }

            retain_item
        });

        children.nodes_mut().insert(
            idx,
            KdlNode::parse(&format!(
                "{}    {} {}\n",
                if idx == 0 { "\n" } else { "" },
                kdl_alias,
                kdl_server
            ))?,
        );

        return Ok(());
    }

    // Case 2: there is no aliases field, so insert it its standard position (second last item
    // before rcon, coincidentally making the logic simple)
    let nodes = document.nodes_mut();

    let (mut idx, mut preceding_node, mut succeeding_node) = (0, None, None);

    for node in nodes.iter() {
        // prefer matches here as the rhs represents all of the nodes that go after this one
        if matches!(node.name().value(), "rcon") {
            succeeding_node = Some(node);
            break;
        }

        idx += 1;
        preceding_node = Some(node);
    }

    let leading_newlines_needed = if let Some(preceding) = preceding_node {
        preceding
            .format()
            .map(|f| f.trailing.chars().rev().take_while(|c| *c == '\n').count())
            .map(|n| 1usize.saturating_sub(n))
            .unwrap_or(1)
    } else {
        0
    };

    let trailing_newlines_needed = if let Some(succeeding) = succeeding_node {
        succeeding
            .format()
            .map(|f| f.leading.chars().take_while(|c| *c == '\n').count())
            .map(|n| 2usize.saturating_sub(n))
            .unwrap_or(2)
    } else {
        0
    };

    nodes.insert(
        idx,
        KdlNode::parse(&format!(
            "{}aliases {{\n    {} {}\n}}{}",
            "\n".repeat(leading_newlines_needed),
            kdl_alias,
            kdl_server,
            "\n".repeat(trailing_newlines_needed)
        ))?,
    );

    Ok(())
}

#[cfg(test)]
mod test {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn paths() {
        let expected_config_dir =
            PathBuf::from(shellexpand::tilde("~/.config/mcserver/").into_owned());

        assert_eq!(
            &get_directory().expect("Failed to get config directory"),
            &expected_config_dir
        );
    }

    fn test_str_transformed_and_parsed(s: &str) {
        KdlNode::parse(&transform_to_kdl_string(s)).expect("Expected parse success");
    }

    #[test]
    fn utilities() {
        assert_eq!(
            transform_number(
                &KdlNode::parse("number 69").expect("Failed to parse number test node"),
                u8::try_from,
                None
            )
            .expect("Failed to transform number"),
            69
        );

        assert_eq!(transform_to_kdl_string("0.1"), "\"0.1\"");

        assert_eq!(
            transform_to_kdl_string("a-w+o-d+j-o+i-d+j-o+i-a+w-d"),
            "a-w+o-d+j-o+i-d+j-o+i-a+w-d"
        );

        test_str_transformed_and_parsed("hello\nworld");
        test_str_transformed_and_parsed("1.7");
        test_str_transformed_and_parsed("{}0#");
        test_str_transformed_and_parsed("bye bye-\r69420");
    }

    #[test]
    fn aliasing() {
        let parsed_alias = parse_alias(
            &KdlNode::parse("default my-server").expect("Failed to parse alias test node"),
        )
        .expect("Failed to parse alias");

        assert_eq!(
            (parsed_alias.0.as_str(), parsed_alias.1.as_str()),
            ("default", "my-server")
        );
    }

    #[test]
    fn file_parsing() {
        let dir = tempdir().expect("Unable to create a temporary directory for testing");

        let path = dir.path();

        load_or_create(path).expect("Failed to create default config");
    }
}
