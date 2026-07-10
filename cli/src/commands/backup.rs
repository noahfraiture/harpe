use std::io::Write;

use futures_util::StreamExt;
use harpe_proto::pb::{ExportGameRequest, memory_service_client::MemoryServiceClient};
use tonic::transport::Channel;

use crate::config::required_config_value;
use crate::output::{backup_chunk_json, game_snapshot_json, write_json, write_path_result};
use crate::{BackupArgs, BackupCommand, CliResult, ClientConfig, with_user};

pub(crate) async fn backup<W: Write>(
    channel: Channel,
    args: BackupArgs,
    user_id: String,
    config: &ClientConfig,
    as_json: bool,
    writer: &mut W,
) -> CliResult<()> {
    let mut client = MemoryServiceClient::new(channel);
    match args.command {
        BackupCommand::Export { game, out } => {
            let game =
                required_config_value("game id", game.as_deref().or(config.game_id.as_deref()))?;
            let response = client
                .export_game(with_user(ExportGameRequest { game_id: game }, &user_id)?)
                .await?
                .into_inner();
            let value = game_snapshot_json(&response);
            if let Some(path) = out {
                std::fs::write(
                    &path,
                    format!("{}\n", serde_json::to_string_pretty(&value)?),
                )?;
                write_path_result(writer, as_json, "backup_path", &path)
            } else {
                write_json(writer, &value)
            }
        }
        BackupCommand::Stream { game, out } => {
            let game =
                required_config_value("game id", game.as_deref().or(config.game_id.as_deref()))?;
            let mut stream = client
                .export_game_stream(with_user(ExportGameRequest { game_id: game }, &user_id)?)
                .await?
                .into_inner();
            if let Some(path) = out {
                let file = std::fs::File::create(&path)?;
                let mut file = std::io::BufWriter::new(file);
                while let Some(next) = stream.next().await {
                    let chunk = next?;
                    writeln!(
                        file,
                        "{}",
                        serde_json::to_string(&backup_chunk_json(&chunk))?
                    )?;
                    if chunk.done {
                        break;
                    }
                }
                file.flush()?;
                write_path_result(writer, as_json, "backup_stream_path", &path)
            } else {
                while let Some(next) = stream.next().await {
                    let chunk = next?;
                    writeln!(
                        writer,
                        "{}",
                        serde_json::to_string(&backup_chunk_json(&chunk))?
                    )?;
                    if chunk.done {
                        break;
                    }
                }
                Ok(())
            }
        }
    }
}
