use std::io::{BufRead, Write};
use std::path::Path;

use futures_util::StreamExt;
use harpe_proto::pb::{
    self, CreateGameRequest, CreateSessionRequest, CreateUserRequest, ExportGameRequest,
    ExportMetricsRequest, GetCharacterRequest, GetGameRequest, GetMetricsRequest,
    GetSessionRequest, GetStorySummaryRequest, GetUserRequest, HealthCheckRequest,
    ListBackgroundJobsRequest, ListCharactersRequest, ListEventsRequest, ListGamesRequest,
    ListLocationsRequest, ListMemoryChunksRequest, ListMessagesRequest, ListSessionsRequest,
    ListWorldFactsRequest, MetricsExportFormat, PreviewContextRequest, SearchMemoryRequest,
    SendMessageRequest, admin_service_client::AdminServiceClient,
    game_service_client::GameServiceClient, health_service_client::HealthServiceClient,
    memory_service_client::MemoryServiceClient, metrics_service_client::MetricsServiceClient,
    session_service_client::SessionServiceClient, user_service_client::UserServiceClient,
};
use serde_json::json;
use tonic::transport::Channel;

use crate::args::admin_status_filter;
use crate::config::{
    DEFAULT_ADDR, read_prompt, required_config_value, required_value, save_config_to_path,
};
use crate::output::*;
use crate::{
    AdminArgs, AdminCommand, BackupArgs, BackupCommand, CliResult, ClientConfig, ConfigArgs,
    ConfigCommand, ConfigKey, GameArgs, GameCommand, HealthArgs, MemoryArgs, MemoryCommand,
    MetricsArgs, MetricsCommand, PlayArgs, SessionArgs, SessionCommand, UserArgs, UserCommand,
    invalid_input, join_words, with_user,
};

mod admin;
mod backup;
mod config;
mod game;
mod health;
mod memory;
mod metrics;
mod play;
mod session;
mod user;

pub(crate) use admin::admin;
pub(crate) use backup::backup;
pub(crate) use config::config;
pub(crate) use game::game;
pub(crate) use health::health;
pub(crate) use memory::memory;
pub(crate) use metrics::metrics;
pub(crate) use play::play;
pub(crate) use session::session;
pub(crate) use user::user;

pub(crate) fn normalize_optional_model(model: Option<String>) -> String {
    model
        .map(|model| model.trim().to_owned())
        .filter(|model| !model.is_empty())
        .unwrap_or_default()
}

fn write_play_help<W: Write>(writer: &mut W) -> CliResult<()> {
    writeln!(
        writer,
        "commands: /context <message>, /summary, /characters, /events, /memory <query>, /help, /quit"
    )?;
    Ok(())
}
