use std::io::Write;
use std::path::Path;

use crate::config::{DEFAULT_ADDR, required_value, save_config_to_path};
use crate::output::{config_json, write_json};
use crate::{CliResult, ClientConfig, ConfigArgs, ConfigCommand, ConfigKey};

pub(crate) fn config<W: Write>(
    args: ConfigArgs,
    config_path: &Path,
    client_config: &mut ClientConfig,
    as_json: bool,
    writer: &mut W,
) -> CliResult<()> {
    match args.command {
        ConfigCommand::Show => {
            let value = config_json(client_config, config_path);
            if as_json {
                write_json(writer, &value)
            } else {
                writeln!(writer, "path={}", config_path.display())?;
                writeln!(
                    writer,
                    "addr={}",
                    client_config.addr.as_deref().unwrap_or(DEFAULT_ADDR)
                )?;
                writeln!(
                    writer,
                    "user_id={}",
                    client_config.user_id.as_deref().unwrap_or("")
                )?;
                writeln!(
                    writer,
                    "game_id={}",
                    client_config.game_id.as_deref().unwrap_or("")
                )?;
                writeln!(
                    writer,
                    "session_id={}",
                    client_config.session_id.as_deref().unwrap_or("")
                )?;
                Ok(())
            }
        }
        ConfigCommand::SetAddr { addr } => {
            client_config.addr = Some(crate::normalize_addr(&addr)?);
            save_config_to_path(config_path, client_config)?;
            write_config_update(writer, as_json, config_path, client_config)
        }
        ConfigCommand::SetUser { user_id } => {
            client_config.user_id = Some(required_value("user id", &user_id)?);
            save_config_to_path(config_path, client_config)?;
            write_config_update(writer, as_json, config_path, client_config)
        }
        ConfigCommand::SetGame { game_id } => {
            client_config.game_id = Some(required_value("game id", &game_id)?);
            save_config_to_path(config_path, client_config)?;
            write_config_update(writer, as_json, config_path, client_config)
        }
        ConfigCommand::SetSession { session_id } => {
            client_config.session_id = Some(required_value("session id", &session_id)?);
            save_config_to_path(config_path, client_config)?;
            write_config_update(writer, as_json, config_path, client_config)
        }
        ConfigCommand::Clear { key } => {
            match key {
                ConfigKey::Addr => client_config.addr = None,
                ConfigKey::User => client_config.user_id = None,
                ConfigKey::Game => client_config.game_id = None,
                ConfigKey::Session => client_config.session_id = None,
                ConfigKey::All => *client_config = ClientConfig::default(),
            }
            save_config_to_path(config_path, client_config)?;
            write_config_update(writer, as_json, config_path, client_config)
        }
    }
}

fn write_config_update<W: Write>(
    writer: &mut W,
    as_json: bool,
    config_path: &Path,
    client_config: &ClientConfig,
) -> CliResult<()> {
    if as_json {
        write_json(writer, &config_json(client_config, config_path))
    } else {
        writeln!(writer, "config_path={}", config_path.display())?;
        Ok(())
    }
}
