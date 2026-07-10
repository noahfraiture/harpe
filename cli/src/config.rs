use std::error::Error;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::CliResult;

pub(crate) const DEFAULT_ADDR: &str = "http://[::1]:50051";
pub(crate) const DEFAULT_CONFIG_FILE: &str = "config.toml";
pub(crate) const LEGACY_CONFIG_FILE: &str = "config.json";

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClientConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub addr: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub game_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
}

pub(crate) fn config_path(explicit_path: Option<&Path>) -> CliResult<PathBuf> {
    if let Some(path) = explicit_path {
        return Ok(path.to_path_buf());
    }

    if let Some(config_home) = std::env::var_os("XDG_CONFIG_HOME") {
        return Ok(PathBuf::from(config_home)
            .join("harpe")
            .join(DEFAULT_CONFIG_FILE));
    }

    if let Some(home) = std::env::var_os("HOME") {
        return Ok(PathBuf::from(home)
            .join(".config")
            .join("harpe")
            .join(DEFAULT_CONFIG_FILE));
    }

    Err(invalid_input(
        "cannot resolve config path; set --config or HOME",
    ))
}

pub(crate) fn load_config_from_path(path: &Path) -> CliResult<ClientConfig> {
    if !path.exists()
        && path.file_name().and_then(|name| name.to_str()) == Some(DEFAULT_CONFIG_FILE)
    {
        let legacy_path = path.with_file_name(LEGACY_CONFIG_FILE);
        if legacy_path.exists() {
            return load_config_file(&legacy_path);
        }
    }

    if !path.exists() {
        return Ok(ClientConfig::default());
    }

    load_config_file(path)
}

fn load_config_file(path: &Path) -> CliResult<ClientConfig> {
    let content = std::fs::read_to_string(path)?;

    if content.trim().is_empty() {
        return Ok(ClientConfig::default());
    }

    if is_json_path(path) {
        Ok(serde_json::from_str(&content)?)
    } else {
        Ok(toml::from_str(&content)?)
    }
}

pub(crate) fn save_config_to_path(path: &Path, config: &ClientConfig) -> CliResult<()> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent)?;
    }
    let content = if is_json_path(path) {
        serde_json::to_string_pretty(config)?
    } else {
        toml::to_string_pretty(config)?
    };
    std::fs::write(path, format!("{content}\n"))?;
    Ok(())
}

fn is_json_path(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("json"))
}

pub(crate) fn resolve_addr(cli_addr: Option<&str>, config: &ClientConfig) -> CliResult<String> {
    normalize_addr(cli_addr.or(config.addr.as_deref()).unwrap_or(DEFAULT_ADDR))
}

pub(crate) fn resolve_user_id<'a>(
    cli_user_id: Option<&'a str>,
    config: &'a ClientConfig,
) -> Option<&'a str> {
    cli_user_id.or(config.user_id.as_deref())
}

pub(crate) fn required_user_id(user_id: Option<&str>) -> CliResult<String> {
    user_id
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .ok_or_else(|| invalid_input("set --user-id or HARPE_USER_ID for this command"))
}

pub(crate) fn required_value(name: &str, value: &str) -> CliResult<String> {
    let value = value.trim();
    if value.is_empty() {
        return Err(invalid_input(format!("{name} is required")));
    }
    Ok(value.to_owned())
}

pub(crate) fn required_config_value(name: &str, value: Option<&str>) -> CliResult<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .ok_or_else(|| invalid_input(format!("set {name} in config or pass it explicitly")))
}

pub(crate) fn normalize_optional_model(model: Option<String>) -> String {
    model
        .map(|model| model.trim().to_owned())
        .filter(|model| !model.is_empty())
        .unwrap_or_default()
}

pub fn normalize_addr(addr: &str) -> CliResult<String> {
    let addr = addr.trim();
    if addr.is_empty() {
        return Err(invalid_input("gRPC address is required"));
    }
    if addr.starts_with("http://") || addr.starts_with("https://") {
        Ok(addr.to_owned())
    } else {
        Ok(format!("http://{addr}"))
    }
}

pub(crate) fn read_prompt(
    system_prompt: String,
    system_prompt_file: Option<PathBuf>,
) -> CliResult<String> {
    match (system_prompt.trim().is_empty(), system_prompt_file) {
        (true, Some(path)) => Ok(std::fs::read_to_string(path)?),
        (false, Some(_)) => Err(invalid_input(
            "use either --system-prompt or --system-prompt-file, not both",
        )),
        (true, None) => Ok(String::new()),
        (false, None) => Ok(system_prompt),
    }
}

pub(crate) fn invalid_input(message: impl Into<String>) -> Box<dyn Error + Send + Sync> {
    Box::new(io::Error::new(io::ErrorKind::InvalidInput, message.into()))
}
