use std::error::Error;
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};

use clap::{Args, Parser, Subcommand, ValueEnum};
use futures_util::StreamExt;
use harpe_server::pb::{
    self, AdminJobStatus, BackgroundJobDebug, Character, ContextMessage, CreateGameRequest,
    CreateSessionRequest, CreateUserRequest, Event, ExportGameRequest, ExportMetricsRequest, Game,
    GameBackupChunk, GameSnapshot, GetCharacterRequest, GetGameRequest, GetMetricsRequest,
    GetSessionRequest, GetStorySummaryRequest, GetUserRequest, HealthCheckRequest,
    HealthCheckResponse, HistogramBucket, ListBackgroundJobsRequest, ListCharactersRequest,
    ListEventsRequest, ListGamesRequest, ListLocationsRequest, ListMemoryChunksRequest,
    ListMessagesRequest, ListSessionsRequest, ListWorldFactsRequest, Location, MemoryChunk,
    MemoryHit, Message, MessageDelta, MetricsExportFormat, MetricsSnapshot, PageInfo, PageRequest,
    PreviewContextRequest, SearchMemoryRequest, SendMessageRequest, Session, StorySummary, User,
    WorldFact, admin_service_client::AdminServiceClient, game_service_client::GameServiceClient,
    health_service_client::HealthServiceClient, memory_service_client::MemoryServiceClient,
    metrics_service_client::MetricsServiceClient, session_service_client::SessionServiceClient,
    user_service_client::UserServiceClient,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tonic::Request;
use tonic::metadata::MetadataValue;
use tonic::transport::{Channel, Endpoint};

pub type CliResult<T> = Result<T, Box<dyn Error + Send + Sync>>;

const DEFAULT_ADDR: &str = "http://[::1]:50051";

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

#[derive(Parser, Debug, Clone, PartialEq)]
#[command(name = "harpe")]
#[command(about = "Command line client for the Harpe roleplay backend")]
pub struct Cli {
    #[arg(long, global = true, env = "HARPE_GRPC_ADDR")]
    pub addr: Option<String>,
    #[arg(long, global = true, env = "HARPE_USER_ID")]
    pub user_id: Option<String>,
    #[arg(long, global = true, env = "HARPE_CONFIG")]
    pub config: Option<PathBuf>,
    #[arg(long, global = true)]
    pub json: bool,
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand, Debug, Clone, PartialEq)]
pub enum Command {
    Health(HealthArgs),
    Metrics(MetricsArgs),
    User(UserArgs),
    Game(GameArgs),
    Session(SessionArgs),
    Memory(MemoryArgs),
    Backup(BackupArgs),
    Admin(AdminArgs),
    Config(ConfigArgs),
    Play(PlayArgs),
}

#[derive(Args, Debug, Clone, PartialEq, Eq)]
pub struct ConfigArgs {
    #[command(subcommand)]
    pub command: ConfigCommand,
}

#[derive(Subcommand, Debug, Clone, PartialEq, Eq)]
pub enum ConfigCommand {
    Show,
    SetAddr {
        addr: String,
    },
    SetUser {
        user_id: String,
    },
    SetGame {
        game_id: String,
    },
    SetSession {
        session_id: String,
    },
    Clear {
        #[arg(value_enum)]
        key: ConfigKey,
    },
}

#[derive(ValueEnum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigKey {
    Addr,
    User,
    Game,
    Session,
    All,
}

#[derive(Args, Debug, Clone, PartialEq, Eq)]
pub struct PlayArgs {
    pub session_id: Option<String>,
}

#[derive(Args, Debug, Clone, PartialEq, Eq)]
pub struct HealthArgs {
    #[arg(long, default_value = "")]
    pub service: String,
}

#[derive(Args, Debug, Clone, PartialEq, Eq)]
pub struct MetricsArgs {
    #[command(subcommand)]
    pub command: Option<MetricsCommand>,
}

#[derive(Subcommand, Debug, Clone, PartialEq, Eq)]
pub enum MetricsCommand {
    Export {
        #[arg(long)]
        out: Option<PathBuf>,
    },
}

#[derive(Args, Debug, Clone, PartialEq, Eq)]
pub struct UserArgs {
    #[command(subcommand)]
    pub command: UserCommand,
}

#[derive(Subcommand, Debug, Clone, PartialEq, Eq)]
pub enum UserCommand {
    Create {
        #[arg(long)]
        name: String,
    },
    Get {
        user_id: String,
    },
}

#[derive(Args, Debug, Clone, PartialEq, Eq)]
pub struct GameArgs {
    #[command(subcommand)]
    pub command: GameCommand,
}

#[derive(Subcommand, Debug, Clone, PartialEq, Eq)]
pub enum GameCommand {
    Create {
        #[arg(long)]
        title: String,
        #[arg(long, default_value = "")]
        system_prompt: String,
        #[arg(long)]
        system_prompt_file: Option<PathBuf>,
    },
    List {
        #[command(flatten)]
        page: PageArgs,
    },
    Get {
        game_id: String,
    },
}

#[derive(Args, Debug, Clone, PartialEq, Eq)]
pub struct SessionArgs {
    #[command(subcommand)]
    pub command: SessionCommand,
}

#[derive(Subcommand, Debug, Clone, PartialEq, Eq)]
pub enum SessionCommand {
    Create {
        #[arg(long)]
        game: Option<String>,
        #[arg(long)]
        title: String,
    },
    List {
        #[arg(long)]
        game: Option<String>,
        #[command(flatten)]
        page: PageArgs,
    },
    Get {
        session_id: String,
    },
    Messages {
        session_id: String,
        #[command(flatten)]
        page: PageArgs,
    },
    Context {
        session_id: String,
        #[arg(required = true, num_args = 1..)]
        content: Vec<String>,
    },
    Send {
        session_id: String,
        #[arg(required = true, num_args = 1..)]
        content: Vec<String>,
    },
}

#[derive(Args, Debug, Clone, PartialEq, Eq)]
pub struct MemoryArgs {
    #[command(subcommand)]
    pub command: MemoryCommand,
}

#[derive(Subcommand, Debug, Clone, PartialEq, Eq)]
pub enum MemoryCommand {
    Summary {
        session_id: String,
    },
    Characters {
        #[arg(long)]
        game: Option<String>,
        #[command(flatten)]
        page: PageArgs,
    },
    Character {
        character_id: String,
    },
    Events {
        session_id: String,
        #[command(flatten)]
        page: PageArgs,
    },
    Facts {
        #[arg(long)]
        game: Option<String>,
        #[command(flatten)]
        page: PageArgs,
    },
    Locations {
        #[arg(long)]
        game: Option<String>,
        #[command(flatten)]
        page: PageArgs,
    },
    Search {
        session_id: String,
        #[arg(required = true, num_args = 1..)]
        query: Vec<String>,
        #[command(flatten)]
        page: PageArgs,
    },
}

#[derive(Args, Debug, Clone, PartialEq, Eq)]
pub struct BackupArgs {
    #[command(subcommand)]
    pub command: BackupCommand,
}

#[derive(Subcommand, Debug, Clone, PartialEq, Eq)]
pub enum BackupCommand {
    Export {
        #[arg(long)]
        game: Option<String>,
        #[arg(long)]
        out: Option<PathBuf>,
    },
    Stream {
        #[arg(long)]
        game: Option<String>,
        #[arg(long)]
        out: Option<PathBuf>,
    },
}

#[derive(Args, Debug, Clone, PartialEq, Eq)]
pub struct AdminArgs {
    #[command(subcommand)]
    pub command: AdminCommand,
}

#[derive(Subcommand, Debug, Clone, PartialEq, Eq)]
pub enum AdminCommand {
    Jobs {
        #[arg(long, value_enum, default_value_t = JobStatusArg::All)]
        status: JobStatusArg,
        #[command(flatten)]
        page: PageArgs,
    },
    RetryJob {
        job_id: String,
        #[arg(long)]
        max_attempts: Option<i32>,
    },
    PurgeJob {
        job_id: String,
    },
    MemoryChunks {
        session_id: String,
        #[command(flatten)]
        page: PageArgs,
    },
}

#[derive(Args, Debug, Clone, PartialEq, Eq)]
pub struct PageArgs {
    #[arg(long, default_value_t = 20)]
    pub limit: u32,
    #[arg(long)]
    pub page_token: Option<String>,
}

#[derive(ValueEnum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobStatusArg {
    All,
    Pending,
    Running,
    Succeeded,
    Failed,
}

impl PageArgs {
    fn request(&self) -> PageRequest {
        PageRequest {
            page_size: self.limit,
            page_token: self.page_token.clone().unwrap_or_default(),
        }
    }
}

pub async fn run(cli: Cli) -> CliResult<()> {
    let stdin = io::stdin();
    let stdout = io::stdout();
    let stdin = stdin.lock();
    let mut stdout = stdout.lock();
    execute_with_io(cli, stdin, &mut stdout).await
}

pub async fn execute<W: Write>(cli: Cli, writer: &mut W) -> CliResult<()> {
    execute_with_io(cli, io::empty(), writer).await
}

pub async fn execute_with_io<R: BufRead, W: Write>(
    cli: Cli,
    reader: R,
    writer: &mut W,
) -> CliResult<()> {
    let config_path = config_path(cli.config.as_deref())?;
    let mut client_config = load_config_from_path(&config_path)?;
    let as_json = cli.json;
    let command = cli.command;

    if let Command::Config(args) = &command {
        return config(
            args.clone(),
            &config_path,
            &mut client_config,
            as_json,
            writer,
        );
    }

    let addr = resolve_addr(cli.addr.as_deref(), &client_config)?;
    let owned_command_user_id = match &command {
        Command::Game(_)
        | Command::Session(_)
        | Command::Memory(_)
        | Command::Backup(_)
        | Command::Play(_) => Some(required_user_id(resolve_user_id(
            cli.user_id.as_deref(),
            &client_config,
        ))?),
        Command::Health(_)
        | Command::Metrics(_)
        | Command::User(_)
        | Command::Admin(_)
        | Command::Config(_) => None,
    };
    let channel = Endpoint::from_shared(addr)?.connect().await?;

    match command {
        Command::Health(args) => health(channel, args, as_json, writer).await,
        Command::Metrics(args) => metrics(channel, args, as_json, writer).await,
        Command::User(args) => user(channel, args, as_json, writer).await,
        Command::Game(args) => {
            game(
                channel,
                args,
                owned_command_user_id.expect("owned commands are validated before connect"),
                as_json,
                writer,
            )
            .await
        }
        Command::Session(args) => {
            session(
                channel,
                args,
                owned_command_user_id.expect("owned commands are validated before connect"),
                &client_config,
                as_json,
                writer,
            )
            .await
        }
        Command::Memory(args) => {
            memory(
                channel,
                args,
                owned_command_user_id.expect("owned commands are validated before connect"),
                &client_config,
                as_json,
                writer,
            )
            .await
        }
        Command::Backup(args) => {
            backup(
                channel,
                args,
                owned_command_user_id.expect("owned commands are validated before connect"),
                &client_config,
                as_json,
                writer,
            )
            .await
        }
        Command::Admin(args) => admin(channel, args, as_json, writer).await,
        Command::Config(_) => unreachable!("config commands return before connecting"),
        Command::Play(args) => {
            play(
                channel,
                args,
                owned_command_user_id.expect("owned commands are validated before connect"),
                &client_config,
                as_json,
                reader,
                writer,
            )
            .await
        }
    }
}

fn config<W: Write>(
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
            client_config.addr = Some(normalize_addr(&addr)?);
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

fn config_json(client_config: &ClientConfig, config_path: &Path) -> Value {
    json!({
        "path": config_path.display().to_string(),
        "addr": client_config.addr,
        "user_id": client_config.user_id,
        "game_id": client_config.game_id,
        "session_id": client_config.session_id,
    })
}

async fn health<W: Write>(
    channel: Channel,
    args: HealthArgs,
    as_json: bool,
    writer: &mut W,
) -> CliResult<()> {
    let response = HealthServiceClient::new(channel)
        .check(HealthCheckRequest {
            service: args.service,
        })
        .await?
        .into_inner();

    if as_json {
        return write_json(writer, &health_json(&response));
    }

    writeln!(
        writer,
        "{} status={} database_ok={} pending_jobs={} failed_jobs={} checked_at={}",
        response.service,
        serving_status_name(response.status),
        response.database_ok,
        response.pending_jobs,
        response.failed_jobs,
        response.checked_at
    )?;
    Ok(())
}

async fn metrics<W: Write>(
    channel: Channel,
    args: MetricsArgs,
    as_json: bool,
    writer: &mut W,
) -> CliResult<()> {
    match args.command {
        None => {
            let response = MetricsServiceClient::new(channel)
                .get_metrics(GetMetricsRequest {})
                .await?
                .into_inner();
            if as_json {
                write_json(writer, &metrics_json(&response))
            } else {
                writeln!(
                    writer,
                    "grpc_requests={} grpc_failures={} streamed_messages={} jobs_processed={} jobs_succeeded={} jobs_retried={} jobs_failed={} health_checks={} collected_at={}",
                    response.grpc_requests,
                    response.grpc_failures,
                    response.streamed_messages,
                    response.jobs_processed,
                    response.jobs_succeeded,
                    response.jobs_retried,
                    response.jobs_failed,
                    response.health_checks,
                    response.collected_at
                )?;
                Ok(())
            }
        }
        Some(MetricsCommand::Export { out }) => {
            let response = MetricsServiceClient::new(channel)
                .export_metrics(ExportMetricsRequest {
                    format: MetricsExportFormat::PrometheusText as i32,
                })
                .await?
                .into_inner();
            if let Some(path) = out {
                std::fs::write(&path, response.body)?;
                write_path_result(writer, as_json, "metrics_path", &path)
            } else {
                write!(writer, "{}", response.body)?;
                Ok(())
            }
        }
    }
}

async fn user<W: Write>(
    channel: Channel,
    args: UserArgs,
    as_json: bool,
    writer: &mut W,
) -> CliResult<()> {
    let mut client = UserServiceClient::new(channel);
    match args.command {
        UserCommand::Create { name } => {
            let response = client
                .create_user(CreateUserRequest { display_name: name })
                .await?
                .into_inner();
            write_user(writer, as_json, &response)
        }
        UserCommand::Get { user_id } => {
            let response = client
                .get_user(GetUserRequest { user_id })
                .await?
                .into_inner();
            write_user(writer, as_json, &response)
        }
    }
}

async fn game<W: Write>(
    channel: Channel,
    args: GameArgs,
    user_id: String,
    as_json: bool,
    writer: &mut W,
) -> CliResult<()> {
    let mut client = GameServiceClient::new(channel);
    match args.command {
        GameCommand::Create {
            title,
            system_prompt,
            system_prompt_file,
        } => {
            let response = client
                .create_game(with_user(
                    CreateGameRequest {
                        title,
                        system_prompt: read_prompt(system_prompt, system_prompt_file)?,
                        owner_user_id: String::new(),
                    },
                    &user_id,
                )?)
                .await?
                .into_inner();
            write_game(writer, as_json, &response)
        }
        GameCommand::List { page } => {
            let response = client
                .list_games(with_user(
                    ListGamesRequest {
                        limit: 0,
                        page: Some(page.request()),
                    },
                    &user_id,
                )?)
                .await?
                .into_inner();
            if as_json {
                write_json(
                    writer,
                    &json!({
                        "games": response.games.iter().map(game_json).collect::<Vec<_>>(),
                        "page": page_json(response.page.as_ref()),
                    }),
                )
            } else {
                for game in response.games {
                    writeln!(
                        writer,
                        "{}\t{}\towner={}\tcreated_at={}",
                        game.id, game.title, game.owner_user_id, game.created_at
                    )?;
                }
                Ok(())
            }
        }
        GameCommand::Get { game_id } => {
            let response = client
                .get_game(with_user(GetGameRequest { game_id }, &user_id)?)
                .await?
                .into_inner();
            write_game(writer, as_json, &response)
        }
    }
}

async fn session<W: Write>(
    channel: Channel,
    args: SessionArgs,
    user_id: String,
    config: &ClientConfig,
    as_json: bool,
    writer: &mut W,
) -> CliResult<()> {
    let mut client = SessionServiceClient::new(channel);
    match args.command {
        SessionCommand::Create { game, title } => {
            let game =
                required_config_value("game id", game.as_deref().or(config.game_id.as_deref()))?;
            let response = client
                .create_session(with_user(
                    CreateSessionRequest {
                        game_id: game,
                        title,
                    },
                    &user_id,
                )?)
                .await?
                .into_inner();
            write_session(writer, as_json, &response)
        }
        SessionCommand::List { game, page } => {
            let game =
                required_config_value("game id", game.as_deref().or(config.game_id.as_deref()))?;
            let response = client
                .list_sessions(with_user(
                    ListSessionsRequest {
                        game_id: game,
                        limit: 0,
                        page: Some(page.request()),
                    },
                    &user_id,
                )?)
                .await?
                .into_inner();
            if as_json {
                write_json(
                    writer,
                    &json!({
                        "sessions": response.sessions.iter().map(session_json).collect::<Vec<_>>(),
                        "page": page_json(response.page.as_ref()),
                    }),
                )
            } else {
                for session in response.sessions {
                    writeln!(
                        writer,
                        "{}\t{}\tgame={}\tcreated_at={}",
                        session.id, session.title, session.game_id, session.created_at
                    )?;
                }
                Ok(())
            }
        }
        SessionCommand::Get { session_id } => {
            let response = client
                .get_session(with_user(GetSessionRequest { session_id }, &user_id)?)
                .await?
                .into_inner();
            write_session(writer, as_json, &response)
        }
        SessionCommand::Messages { session_id, page } => {
            let response = client
                .list_messages(with_user(
                    ListMessagesRequest {
                        session_id,
                        limit: 0,
                        page: Some(page.request()),
                    },
                    &user_id,
                )?)
                .await?
                .into_inner();
            if as_json {
                write_json(
                    writer,
                    &json!({
                        "messages": response.messages.iter().map(message_json).collect::<Vec<_>>(),
                        "page": page_json(response.page.as_ref()),
                    }),
                )
            } else {
                for message in response.messages {
                    writeln!(
                        writer,
                        "[{}] {}: {}",
                        message.created_at,
                        role_name(message.role),
                        message.content
                    )?;
                }
                Ok(())
            }
        }
        SessionCommand::Context {
            session_id,
            content,
        } => {
            let response = client
                .preview_context(with_user(
                    PreviewContextRequest {
                        session_id,
                        content: join_words(content),
                    },
                    &user_id,
                )?)
                .await?
                .into_inner();
            if as_json {
                write_json(
                    writer,
                    &json!({
                        "estimated_tokens": response.estimated_tokens,
                        "messages": response.messages.iter().map(context_message_json).collect::<Vec<_>>(),
                    }),
                )
            } else {
                writeln!(writer, "estimated_tokens={}", response.estimated_tokens)?;
                for message in response.messages {
                    writeln!(
                        writer,
                        "{} [{} tokens]\n{}",
                        role_name(message.role),
                        message.estimated_tokens,
                        message.content
                    )?;
                }
                Ok(())
            }
        }
        SessionCommand::Send {
            session_id,
            content,
        } => {
            send_message(
                client,
                session_id,
                join_words(content),
                user_id,
                as_json,
                writer,
            )
            .await
        }
    }
}

async fn send_message<W: Write>(
    mut client: SessionServiceClient<Channel>,
    session_id: String,
    content: String,
    user_id: String,
    as_json: bool,
    writer: &mut W,
) -> CliResult<()> {
    let mut stream = client
        .send_message(with_user(
            SendMessageRequest {
                session_id,
                content,
            },
            &user_id,
        )?)
        .await?
        .into_inner();
    let mut deltas = Vec::new();
    let mut full_response = String::new();

    while let Some(next) = stream.next().await {
        let delta = next?;
        if as_json {
            full_response.push_str(&delta.delta);
            deltas.push(delta_json(&delta));
        } else if !delta.done {
            write!(writer, "{}", delta.delta)?;
            writer.flush()?;
        }
    }

    if as_json {
        write_json(
            writer,
            &json!({
                "response": full_response,
                "deltas": deltas,
            }),
        )
    } else {
        writeln!(writer)?;
        Ok(())
    }
}

async fn play<R: BufRead, W: Write>(
    channel: Channel,
    args: PlayArgs,
    user_id: String,
    config: &ClientConfig,
    as_json: bool,
    mut reader: R,
    writer: &mut W,
) -> CliResult<()> {
    if as_json {
        return Err(invalid_input("--json is not supported with play"));
    }

    let session_id = required_config_value(
        "session id",
        args.session_id.as_deref().or(config.session_id.as_deref()),
    )?;
    let session = SessionServiceClient::new(channel.clone())
        .get_session(with_user(
            GetSessionRequest {
                session_id: session_id.clone(),
            },
            &user_id,
        )?)
        .await?
        .into_inner();

    writeln!(
        writer,
        "session={} title={} game={}",
        session.id, session.title, session.game_id
    )?;
    write_play_help(writer)?;

    let mut line = String::new();
    loop {
        write!(writer, "> ")?;
        writer.flush()?;
        line.clear();
        if reader.read_line(&mut line)? == 0 {
            break;
        }

        let input = line.trim();
        if input.is_empty() {
            continue;
        }

        match handle_play_input(
            channel.clone(),
            &session.id,
            &session.game_id,
            &user_id,
            input,
            writer,
        )
        .await
        {
            Ok(true) => {}
            Ok(false) => break,
            Err(error) => writeln!(writer, "error: {error}")?,
        }
    }

    Ok(())
}

async fn handle_play_input<W: Write>(
    channel: Channel,
    session_id: &str,
    game_id: &str,
    user_id: &str,
    input: &str,
    writer: &mut W,
) -> CliResult<bool> {
    match input {
        "/quit" | "/exit" => return Ok(false),
        "/help" => {
            write_play_help(writer)?;
            return Ok(true);
        }
        "/summary" => {
            let summary = MemoryServiceClient::new(channel)
                .get_story_summary(with_user(
                    GetStorySummaryRequest {
                        session_id: session_id.to_owned(),
                    },
                    user_id,
                )?)
                .await?
                .into_inner();
            writeln!(writer, "updated_at={}", summary.updated_at)?;
            writeln!(writer, "{}", summary.content)?;
            return Ok(true);
        }
        "/characters" => {
            let characters = MemoryServiceClient::new(channel)
                .list_characters(with_user(
                    ListCharactersRequest {
                        game_id: game_id.to_owned(),
                        limit: 20,
                        page: None,
                    },
                    user_id,
                )?)
                .await?
                .into_inner()
                .characters;
            for character in characters {
                writeln!(
                    writer,
                    "{}\t{}\tstatus={}\t{}",
                    character.id, character.name, character.status, character.description
                )?;
            }
            return Ok(true);
        }
        "/events" => {
            let events = MemoryServiceClient::new(channel)
                .list_events(with_user(
                    ListEventsRequest {
                        session_id: session_id.to_owned(),
                        limit: 20,
                        page: None,
                    },
                    user_id,
                )?)
                .await?
                .into_inner()
                .events;
            for event in events {
                writeln!(
                    writer,
                    "{}\timportance={}\t{}\t{}",
                    event.id, event.importance, event.created_at, event.summary
                )?;
            }
            return Ok(true);
        }
        _ => {}
    }

    if let Some(content) = input.strip_prefix("/context ") {
        let content = required_value("context content", content)?;
        let response = SessionServiceClient::new(channel)
            .preview_context(with_user(
                PreviewContextRequest {
                    session_id: session_id.to_owned(),
                    content,
                },
                user_id,
            )?)
            .await?
            .into_inner();
        writeln!(writer, "estimated_tokens={}", response.estimated_tokens)?;
        for message in response.messages {
            writeln!(
                writer,
                "{} [{} tokens]\n{}",
                role_name(message.role),
                message.estimated_tokens,
                message.content
            )?;
        }
        return Ok(true);
    }

    if let Some(query) = input.strip_prefix("/memory ") {
        let query = required_value("memory query", query)?;
        let hits = MemoryServiceClient::new(channel)
            .search_memory(with_user(
                SearchMemoryRequest {
                    session_id: session_id.to_owned(),
                    query,
                    limit: 10,
                    page: None,
                },
                user_id,
            )?)
            .await?
            .into_inner()
            .hits;
        for hit in hits {
            writeln!(
                writer,
                "{}\tscore={:.4}\tkind={}\t{}",
                hit.id, hit.score, hit.kind, hit.content
            )?;
        }
        return Ok(true);
    }

    if input.starts_with('/') {
        writeln!(writer, "unknown command: {input}")?;
        return Ok(true);
    }

    send_message(
        SessionServiceClient::new(channel),
        session_id.to_owned(),
        input.to_owned(),
        user_id.to_owned(),
        false,
        writer,
    )
    .await?;
    Ok(true)
}

fn write_play_help<W: Write>(writer: &mut W) -> CliResult<()> {
    writeln!(
        writer,
        "commands: /context <message>, /summary, /characters, /events, /memory <query>, /help, /quit"
    )?;
    Ok(())
}

async fn memory<W: Write>(
    channel: Channel,
    args: MemoryArgs,
    user_id: String,
    config: &ClientConfig,
    as_json: bool,
    writer: &mut W,
) -> CliResult<()> {
    let mut client = MemoryServiceClient::new(channel);
    match args.command {
        MemoryCommand::Summary { session_id } => {
            let response = client
                .get_story_summary(with_user(GetStorySummaryRequest { session_id }, &user_id)?)
                .await?
                .into_inner();
            if as_json {
                write_json(writer, &story_summary_json(&response))
            } else {
                writeln!(writer, "updated_at={}", response.updated_at)?;
                writeln!(writer, "{}", response.content)?;
                Ok(())
            }
        }
        MemoryCommand::Characters { game, page } => {
            let game =
                required_config_value("game id", game.as_deref().or(config.game_id.as_deref()))?;
            let response = client
                .list_characters(with_user(
                    ListCharactersRequest {
                        game_id: game,
                        limit: 0,
                        page: Some(page.request()),
                    },
                    &user_id,
                )?)
                .await?
                .into_inner();
            if as_json {
                write_json(
                    writer,
                    &json!({
                        "characters": response.characters.iter().map(character_json).collect::<Vec<_>>(),
                        "page": page_json(response.page.as_ref()),
                    }),
                )
            } else {
                for character in response.characters {
                    writeln!(
                        writer,
                        "{}\t{}\tstatus={}\t{}",
                        character.id, character.name, character.status, character.description
                    )?;
                }
                Ok(())
            }
        }
        MemoryCommand::Character { character_id } => {
            let response = client
                .get_character(with_user(GetCharacterRequest { character_id }, &user_id)?)
                .await?
                .into_inner();
            if as_json {
                write_json(writer, &character_json(&response))
            } else {
                writeln!(
                    writer,
                    "{}\t{}\tstatus={}\n{}",
                    response.id, response.name, response.status, response.description
                )?;
                Ok(())
            }
        }
        MemoryCommand::Events { session_id, page } => {
            let response = client
                .list_events(with_user(
                    ListEventsRequest {
                        session_id,
                        limit: 0,
                        page: Some(page.request()),
                    },
                    &user_id,
                )?)
                .await?
                .into_inner();
            if as_json {
                write_json(
                    writer,
                    &json!({
                        "events": response.events.iter().map(event_json).collect::<Vec<_>>(),
                        "page": page_json(response.page.as_ref()),
                    }),
                )
            } else {
                for event in response.events {
                    writeln!(
                        writer,
                        "{}\timportance={}\t{}\t{}",
                        event.id, event.importance, event.created_at, event.summary
                    )?;
                }
                Ok(())
            }
        }
        MemoryCommand::Facts { game, page } => {
            let game =
                required_config_value("game id", game.as_deref().or(config.game_id.as_deref()))?;
            let response = client
                .list_world_facts(with_user(
                    ListWorldFactsRequest {
                        game_id: game,
                        limit: 0,
                        page: Some(page.request()),
                    },
                    &user_id,
                )?)
                .await?
                .into_inner();
            if as_json {
                write_json(
                    writer,
                    &json!({
                        "facts": response.facts.iter().map(world_fact_json).collect::<Vec<_>>(),
                        "page": page_json(response.page.as_ref()),
                    }),
                )
            } else {
                for fact in response.facts {
                    writeln!(
                        writer,
                        "{}\tconfidence={:.2}\t{}",
                        fact.id, fact.confidence, fact.content
                    )?;
                }
                Ok(())
            }
        }
        MemoryCommand::Locations { game, page } => {
            let game =
                required_config_value("game id", game.as_deref().or(config.game_id.as_deref()))?;
            let response = client
                .list_locations(with_user(
                    ListLocationsRequest {
                        game_id: game,
                        limit: 0,
                        page: Some(page.request()),
                    },
                    &user_id,
                )?)
                .await?
                .into_inner();
            if as_json {
                write_json(
                    writer,
                    &json!({
                        "locations": response.locations.iter().map(location_json).collect::<Vec<_>>(),
                        "page": page_json(response.page.as_ref()),
                    }),
                )
            } else {
                for location in response.locations {
                    writeln!(
                        writer,
                        "{}\t{}\t{}",
                        location.id, location.name, location.description
                    )?;
                }
                Ok(())
            }
        }
        MemoryCommand::Search {
            session_id,
            query,
            page,
        } => {
            let response = client
                .search_memory(with_user(
                    SearchMemoryRequest {
                        session_id,
                        query: join_words(query),
                        limit: 0,
                        page: Some(page.request()),
                    },
                    &user_id,
                )?)
                .await?
                .into_inner();
            if as_json {
                write_json(
                    writer,
                    &json!({
                        "hits": response.hits.iter().map(memory_hit_json).collect::<Vec<_>>(),
                        "page": page_json(response.page.as_ref()),
                    }),
                )
            } else {
                for hit in response.hits {
                    writeln!(
                        writer,
                        "{}\tscore={:.4}\tkind={}\t{}",
                        hit.id, hit.score, hit.kind, hit.content
                    )?;
                }
                Ok(())
            }
        }
    }
}

async fn backup<W: Write>(
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
                let mut file = io::BufWriter::new(file);
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

async fn admin<W: Write>(
    channel: Channel,
    args: AdminArgs,
    as_json: bool,
    writer: &mut W,
) -> CliResult<()> {
    let mut client = AdminServiceClient::new(channel);
    match args.command {
        AdminCommand::Jobs { status, page } => {
            let response = client
                .list_background_jobs(ListBackgroundJobsRequest {
                    status: admin_status_filter(status),
                    limit: 0,
                    page: Some(page.request()),
                })
                .await?
                .into_inner();
            if as_json {
                write_json(
                    writer,
                    &json!({
                        "jobs": response.jobs.iter().map(background_job_json).collect::<Vec<_>>(),
                        "page": page_json(response.page.as_ref()),
                    }),
                )
            } else {
                for job in response.jobs {
                    writeln!(
                        writer,
                        "{}\t{}\tstatus={}\tattempts={}/{}\trun_after={}\t{}",
                        job.id,
                        job_kind_name(job.kind),
                        admin_status_name(job.status),
                        job.attempts,
                        job.max_attempts,
                        job.run_after,
                        job.last_error
                    )?;
                }
                Ok(())
            }
        }
        AdminCommand::RetryJob {
            job_id,
            max_attempts,
        } => {
            let response = client
                .retry_background_job(pb::RetryBackgroundJobRequest {
                    job_id,
                    max_attempts: max_attempts.unwrap_or_default(),
                })
                .await?
                .into_inner();
            write_job(writer, as_json, &response)
        }
        AdminCommand::PurgeJob { job_id } => {
            let response = client
                .purge_background_job(pb::PurgeBackgroundJobRequest { job_id })
                .await?
                .into_inner();
            write_job(writer, as_json, &response)
        }
        AdminCommand::MemoryChunks { session_id, page } => {
            let response = client
                .list_memory_chunks(ListMemoryChunksRequest {
                    session_id,
                    limit: 0,
                    page: Some(page.request()),
                })
                .await?
                .into_inner();
            if as_json {
                write_json(
                    writer,
                    &json!({
                        "chunks": response.chunks.iter().map(memory_chunk_json).collect::<Vec<_>>(),
                        "page": page_json(response.page.as_ref()),
                    }),
                )
            } else {
                for chunk in response.chunks {
                    writeln!(
                        writer,
                        "{}\tkind={}\tembedding_dims={}\t{}",
                        chunk.id,
                        chunk.kind,
                        chunk.embedding.len(),
                        chunk.content
                    )?;
                }
                Ok(())
            }
        }
    }
}

fn write_user<W: Write>(writer: &mut W, as_json: bool, user: &User) -> CliResult<()> {
    if as_json {
        write_json(writer, &user_json(user))
    } else {
        writeln!(
            writer,
            "{}\t{}\tcreated_at={}",
            user.id, user.display_name, user.created_at
        )?;
        Ok(())
    }
}

fn write_game<W: Write>(writer: &mut W, as_json: bool, game: &Game) -> CliResult<()> {
    if as_json {
        write_json(writer, &game_json(game))
    } else {
        writeln!(
            writer,
            "{}\t{}\towner={}\tcreated_at={}",
            game.id, game.title, game.owner_user_id, game.created_at
        )?;
        Ok(())
    }
}

fn write_session<W: Write>(writer: &mut W, as_json: bool, session: &Session) -> CliResult<()> {
    if as_json {
        write_json(writer, &session_json(session))
    } else {
        writeln!(
            writer,
            "{}\t{}\tgame={}\tcreated_at={}",
            session.id, session.title, session.game_id, session.created_at
        )?;
        Ok(())
    }
}

fn write_job<W: Write>(writer: &mut W, as_json: bool, job: &BackgroundJobDebug) -> CliResult<()> {
    if as_json {
        write_json(writer, &background_job_json(job))
    } else {
        writeln!(
            writer,
            "{}\t{}\tstatus={}\tattempts={}/{}",
            job.id,
            job_kind_name(job.kind),
            admin_status_name(job.status),
            job.attempts,
            job.max_attempts
        )?;
        Ok(())
    }
}

fn write_path_result<W: Write>(
    writer: &mut W,
    as_json: bool,
    key: &str,
    path: &Path,
) -> CliResult<()> {
    if as_json {
        write_json(writer, &json!({ key: path.display().to_string() }))
    } else {
        writeln!(writer, "{}={}", key, path.display())?;
        Ok(())
    }
}

fn write_json<W: Write>(writer: &mut W, value: &Value) -> CliResult<()> {
    serde_json::to_writer_pretty(&mut *writer, value)?;
    writeln!(writer)?;
    Ok(())
}

fn with_user<T>(message: T, user_id: &str) -> CliResult<Request<T>> {
    let mut request = Request::new(message);
    request
        .metadata_mut()
        .insert("x-user-id", MetadataValue::try_from(user_id)?);
    Ok(request)
}

fn config_path(explicit_path: Option<&Path>) -> CliResult<PathBuf> {
    if let Some(path) = explicit_path {
        return Ok(path.to_path_buf());
    }

    if let Some(config_home) = std::env::var_os("XDG_CONFIG_HOME") {
        return Ok(PathBuf::from(config_home).join("harpe").join("config.json"));
    }

    if let Some(home) = std::env::var_os("HOME") {
        return Ok(PathBuf::from(home)
            .join(".config")
            .join("harpe")
            .join("config.json"));
    }

    Err(invalid_input(
        "cannot resolve config path; set --config or HOME",
    ))
}

fn load_config_from_path(path: &Path) -> CliResult<ClientConfig> {
    if !path.exists() {
        return Ok(ClientConfig::default());
    }

    let content = std::fs::read_to_string(path)?;
    if content.trim().is_empty() {
        return Ok(ClientConfig::default());
    }

    Ok(serde_json::from_str(&content)?)
}

fn save_config_to_path(path: &Path, config: &ClientConfig) -> CliResult<()> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, format!("{}\n", serde_json::to_string_pretty(config)?))?;
    Ok(())
}

fn resolve_addr(cli_addr: Option<&str>, config: &ClientConfig) -> CliResult<String> {
    normalize_addr(cli_addr.or(config.addr.as_deref()).unwrap_or(DEFAULT_ADDR))
}

fn resolve_user_id<'a>(cli_user_id: Option<&'a str>, config: &'a ClientConfig) -> Option<&'a str> {
    cli_user_id.or(config.user_id.as_deref())
}

fn required_user_id(user_id: Option<&str>) -> CliResult<String> {
    user_id
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .ok_or_else(|| invalid_input("set --user-id or HARPE_USER_ID for this command"))
}

fn required_value(name: &str, value: &str) -> CliResult<String> {
    let value = value.trim();
    if value.is_empty() {
        return Err(invalid_input(format!("{name} is required")));
    }
    Ok(value.to_owned())
}

fn required_config_value(name: &str, value: Option<&str>) -> CliResult<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .ok_or_else(|| invalid_input(format!("set {name} in config or pass it explicitly")))
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

fn read_prompt(system_prompt: String, system_prompt_file: Option<PathBuf>) -> CliResult<String> {
    match (system_prompt.trim().is_empty(), system_prompt_file) {
        (true, Some(path)) => Ok(std::fs::read_to_string(path)?),
        (false, Some(_)) => Err(invalid_input(
            "use either --system-prompt or --system-prompt-file, not both",
        )),
        (true, None) => Ok(String::new()),
        (false, None) => Ok(system_prompt),
    }
}

pub fn join_words(words: Vec<String>) -> String {
    words.join(" ")
}

fn admin_status_filter(status: JobStatusArg) -> i32 {
    match status {
        JobStatusArg::All => AdminJobStatus::Unspecified as i32,
        JobStatusArg::Pending => AdminJobStatus::Pending as i32,
        JobStatusArg::Running => AdminJobStatus::Running as i32,
        JobStatusArg::Succeeded => AdminJobStatus::Succeeded as i32,
        JobStatusArg::Failed => AdminJobStatus::Failed as i32,
    }
}

fn invalid_input(message: impl Into<String>) -> Box<dyn Error + Send + Sync> {
    Box::new(io::Error::new(io::ErrorKind::InvalidInput, message.into()))
}

fn user_json(user: &User) -> Value {
    json!({
        "id": user.id,
        "display_name": user.display_name,
        "created_at": user.created_at,
    })
}

fn game_json(game: &Game) -> Value {
    json!({
        "id": game.id,
        "title": game.title,
        "system_prompt": game.system_prompt,
        "created_at": game.created_at,
        "owner_user_id": game.owner_user_id,
    })
}

fn session_json(session: &Session) -> Value {
    json!({
        "id": session.id,
        "game_id": session.game_id,
        "title": session.title,
        "created_at": session.created_at,
    })
}

fn message_json(message: &Message) -> Value {
    json!({
        "id": message.id,
        "session_id": message.session_id,
        "role": role_name(message.role),
        "content": message.content,
        "created_at": message.created_at,
    })
}

fn context_message_json(message: &ContextMessage) -> Value {
    json!({
        "role": role_name(message.role),
        "content": message.content,
        "estimated_tokens": message.estimated_tokens,
    })
}

fn delta_json(delta: &MessageDelta) -> Value {
    json!({
        "session_id": delta.session_id,
        "message_id": delta.message_id,
        "delta": delta.delta,
        "done": delta.done,
        "sequence": delta.sequence,
        "finish_reason": finish_reason_name(delta.finish_reason),
    })
}

fn story_summary_json(summary: &StorySummary) -> Value {
    json!({
        "session_id": summary.session_id,
        "content": summary.content,
        "updated_at": summary.updated_at,
    })
}

fn character_json(character: &Character) -> Value {
    json!({
        "id": character.id,
        "game_id": character.game_id,
        "name": character.name,
        "description": character.description,
        "status": character.status,
        "updated_at": character.updated_at,
    })
}

fn event_json(event: &Event) -> Value {
    json!({
        "id": event.id,
        "session_id": event.session_id,
        "summary": event.summary,
        "importance": event.importance,
        "created_at": event.created_at,
    })
}

fn world_fact_json(fact: &WorldFact) -> Value {
    json!({
        "id": fact.id,
        "game_id": fact.game_id,
        "subject": fact.subject,
        "predicate": fact.predicate,
        "object": fact.object,
        "content": fact.content,
        "confidence": fact.confidence,
        "updated_at": fact.updated_at,
    })
}

fn location_json(location: &Location) -> Value {
    json!({
        "id": location.id,
        "game_id": location.game_id,
        "name": location.name,
        "description": location.description,
        "updated_at": location.updated_at,
    })
}

fn memory_hit_json(hit: &MemoryHit) -> Value {
    json!({
        "id": hit.id,
        "session_id": hit.session_id,
        "kind": hit.kind,
        "content": hit.content,
        "score": hit.score,
    })
}

fn memory_chunk_json(chunk: &MemoryChunk) -> Value {
    json!({
        "id": chunk.id,
        "session_id": chunk.session_id,
        "kind": chunk.kind,
        "content": chunk.content,
        "embedding_dims": chunk.embedding.len(),
        "embedding": chunk.embedding,
        "created_at": chunk.created_at,
    })
}

fn health_json(health: &HealthCheckResponse) -> Value {
    json!({
        "status": serving_status_name(health.status),
        "service": health.service,
        "version": health.version,
        "database_ok": health.database_ok,
        "pending_jobs": health.pending_jobs,
        "failed_jobs": health.failed_jobs,
        "checked_at": health.checked_at,
    })
}

fn metrics_json(metrics: &MetricsSnapshot) -> Value {
    json!({
        "grpc_requests": metrics.grpc_requests,
        "grpc_failures": metrics.grpc_failures,
        "streamed_messages": metrics.streamed_messages,
        "jobs_processed": metrics.jobs_processed,
        "jobs_succeeded": metrics.jobs_succeeded,
        "jobs_retried": metrics.jobs_retried,
        "jobs_failed": metrics.jobs_failed,
        "health_checks": metrics.health_checks,
        "collected_at": metrics.collected_at,
        "grpc_latency_count": metrics.grpc_latency_count,
        "grpc_latency_sum_ms": metrics.grpc_latency_sum_ms,
        "grpc_latency_buckets": metrics.grpc_latency_buckets.iter().map(histogram_bucket_json).collect::<Vec<_>>(),
    })
}

fn histogram_bucket_json(bucket: &HistogramBucket) -> Value {
    json!({
        "le": bucket.le,
        "count": bucket.count,
    })
}

fn background_job_json(job: &BackgroundJobDebug) -> Value {
    json!({
        "id": job.id,
        "kind": job_kind_name(job.kind),
        "status": admin_status_name(job.status),
        "payload_json": job.payload_json,
        "attempts": job.attempts,
        "max_attempts": job.max_attempts,
        "last_error": job.last_error,
        "run_after": job.run_after,
        "created_at": job.created_at,
        "updated_at": job.updated_at,
    })
}

fn game_snapshot_json(snapshot: &GameSnapshot) -> Value {
    json!({
        "game": snapshot.game.as_ref().map(game_json),
        "sessions": snapshot.sessions.iter().map(session_json).collect::<Vec<_>>(),
        "summaries": snapshot.summaries.iter().map(story_summary_json).collect::<Vec<_>>(),
        "characters": snapshot.characters.iter().map(character_json).collect::<Vec<_>>(),
        "events": snapshot.events.iter().map(event_json).collect::<Vec<_>>(),
        "world_facts": snapshot.world_facts.iter().map(world_fact_json).collect::<Vec<_>>(),
        "locations": snapshot.locations.iter().map(location_json).collect::<Vec<_>>(),
        "memory_chunks": snapshot.memory_chunks.iter().map(memory_chunk_json).collect::<Vec<_>>(),
        "exported_at": snapshot.exported_at,
    })
}

fn backup_chunk_json(chunk: &GameBackupChunk) -> Value {
    let payload = serde_json::from_str::<Value>(&chunk.payload_json)
        .unwrap_or_else(|_| json!({ "raw": chunk.payload_json }));
    json!({
        "game_id": chunk.game_id,
        "kind": chunk.kind,
        "sequence": chunk.sequence,
        "payload": payload,
        "done": chunk.done,
    })
}

fn page_json(page: Option<&PageInfo>) -> Value {
    match page {
        Some(page) => json!({
            "next_page_token": page.next_page_token,
            "returned_count": page.returned_count,
        }),
        None => Value::Null,
    }
}

fn role_name(role: i32) -> &'static str {
    match pb::MessageRole::try_from(role).ok() {
        Some(pb::MessageRole::System) => "system",
        Some(pb::MessageRole::User) => "user",
        Some(pb::MessageRole::Assistant) => "assistant",
        Some(pb::MessageRole::Unspecified) | None => "unspecified",
    }
}

fn finish_reason_name(reason: i32) -> &'static str {
    match pb::MessageFinishReason::try_from(reason).ok() {
        Some(pb::MessageFinishReason::InProgress) => "in_progress",
        Some(pb::MessageFinishReason::AssistantComplete) => "assistant_complete",
        Some(pb::MessageFinishReason::Unspecified) | None => "unspecified",
    }
}

fn serving_status_name(status: i32) -> &'static str {
    match pb::ServingStatus::try_from(status).ok() {
        Some(pb::ServingStatus::Serving) => "serving",
        Some(pb::ServingStatus::Degraded) => "degraded",
        Some(pb::ServingStatus::NotServing) => "not_serving",
        Some(pb::ServingStatus::Unspecified) | None => "unspecified",
    }
}

fn admin_status_name(status: i32) -> &'static str {
    match pb::AdminJobStatus::try_from(status).ok() {
        Some(pb::AdminJobStatus::Pending) => "pending",
        Some(pb::AdminJobStatus::Running) => "running",
        Some(pb::AdminJobStatus::Succeeded) => "succeeded",
        Some(pb::AdminJobStatus::Failed) => "failed",
        Some(pb::AdminJobStatus::Unspecified) | None => "unspecified",
    }
}

fn job_kind_name(kind: i32) -> &'static str {
    match pb::AdminJobKind::try_from(kind).ok() {
        Some(pb::AdminJobKind::UpdateMemoryAfterTurn) => "update_memory_after_turn",
        Some(pb::AdminJobKind::Unspecified) | None => "unspecified",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn parses_global_options_and_session_send_content() {
        let cli = Cli::parse_from([
            "harpe",
            "--addr",
            "127.0.0.1:50051",
            "--user-id",
            "user-1",
            "--json",
            "session",
            "send",
            "session-1",
            "open",
            "the",
            "gate",
        ]);

        assert_eq!(cli.addr.as_deref(), Some("127.0.0.1:50051"));
        assert_eq!(cli.user_id.as_deref(), Some("user-1"));
        assert!(cli.json);
        assert_eq!(
            cli.command,
            Command::Session(SessionArgs {
                command: SessionCommand::Send {
                    session_id: "session-1".to_owned(),
                    content: vec!["open".to_owned(), "the".to_owned(), "gate".to_owned()],
                }
            })
        );
    }

    #[test]
    fn parses_memory_search_with_limit() {
        let cli = Cli::parse_from([
            "harpe",
            "--user-id",
            "user-1",
            "memory",
            "search",
            "session-1",
            "silver",
            "key",
            "--limit",
            "7",
        ]);

        let Command::Memory(memory) = cli.command else {
            panic!("expected memory command");
        };
        let MemoryCommand::Search {
            session_id,
            query,
            page,
        } = memory.command
        else {
            panic!("expected search command");
        };
        assert_eq!(session_id, "session-1");
        assert_eq!(query, vec!["silver".to_owned(), "key".to_owned()]);
        assert_eq!(page.limit, 7);
    }

    #[test]
    fn parses_page_token_for_list_commands() {
        let cli = Cli::parse_from([
            "harpe",
            "--user-id",
            "user-1",
            "game",
            "list",
            "--limit",
            "3",
            "--page-token",
            "cursor-1",
        ]);

        let Command::Game(game) = cli.command else {
            panic!("expected game command");
        };
        let GameCommand::List { page } = game.command else {
            panic!("expected list command");
        };

        assert_eq!(page.limit, 3);
        assert_eq!(page.page_token.as_deref(), Some("cursor-1"));
    }

    #[test]
    fn parses_config_commands() {
        let cli = Cli::parse_from(["harpe", "config", "set-session", "session-1"]);

        assert_eq!(
            cli.command,
            Command::Config(ConfigArgs {
                command: ConfigCommand::SetSession {
                    session_id: "session-1".to_owned(),
                }
            })
        );
    }

    #[test]
    fn client_config_round_trips_and_resolves_defaults() {
        let path = temp_test_path("client-config.json");
        let config = ClientConfig {
            addr: Some("http://127.0.0.1:50051".to_owned()),
            user_id: Some("user-1".to_owned()),
            game_id: Some("game-1".to_owned()),
            session_id: Some("session-1".to_owned()),
        };

        save_config_to_path(&path, &config).unwrap();
        let loaded = load_config_from_path(&path).unwrap();

        assert_eq!(loaded, config);
        assert_eq!(
            resolve_addr(None, &loaded).unwrap(),
            "http://127.0.0.1:50051"
        );
        assert_eq!(resolve_user_id(None, &loaded), Some("user-1"));
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn normalizes_addresses_for_tonic_endpoints() {
        assert_eq!(normalize_addr("[::1]:50051").unwrap(), "http://[::1]:50051");
        assert_eq!(
            normalize_addr("http://127.0.0.1:50051").unwrap(),
            "http://127.0.0.1:50051"
        );
        assert!(
            normalize_addr(" ")
                .unwrap_err()
                .to_string()
                .contains("address")
        );
    }

    #[test]
    fn joins_unquoted_message_words() {
        assert_eq!(
            join_words(vec!["inspect".to_owned(), "the bell".to_owned()]),
            "inspect the bell"
        );
    }

    #[test]
    fn requires_user_id_for_owned_commands() {
        let cli = Cli::parse_from(["harpe", "game", "list"]);
        assert!(
            required_user_id(cli.user_id.as_deref())
                .unwrap_err()
                .to_string()
                .contains("--user-id")
        );

        let cli = Cli::parse_from(["harpe", "--user-id", " user-1 ", "game", "list"]);
        assert_eq!(required_user_id(cli.user_id.as_deref()).unwrap(), "user-1");
    }

    #[tokio::test]
    async fn owned_commands_validate_user_id_before_connecting() {
        let cli = Cli::parse_from(["harpe", "--addr", "http://127.0.0.1:1", "game", "list"]);
        let mut output = Vec::new();

        let error = execute(cli, &mut output).await.unwrap_err();

        assert!(error.to_string().contains("--user-id"));
    }

    #[test]
    fn refuses_prompt_string_and_file_together() {
        let path = PathBuf::from("prompt.txt");
        let error = read_prompt("prompt".to_owned(), Some(path)).unwrap_err();
        assert!(error.to_string().contains("either --system-prompt"));
    }

    #[test]
    fn converts_enums_to_stable_output_names() {
        assert_eq!(role_name(pb::MessageRole::Assistant as i32), "assistant");
        assert_eq!(
            finish_reason_name(pb::MessageFinishReason::AssistantComplete as i32),
            "assistant_complete"
        );
        assert_eq!(
            serving_status_name(pb::ServingStatus::Degraded as i32),
            "degraded"
        );
        assert_eq!(
            admin_status_name(pb::AdminJobStatus::Failed as i32),
            "failed"
        );
    }

    #[test]
    fn json_uses_names_instead_of_numeric_enums() {
        let message = Message {
            id: "message-1".to_owned(),
            session_id: "session-1".to_owned(),
            role: pb::MessageRole::User as i32,
            content: "I knock.".to_owned(),
            created_at: "now".to_owned(),
        };

        assert_eq!(message_json(&message)["role"], "user");
    }

    fn temp_test_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("harpe-cli-unit-{}-{name}", std::process::id()))
    }
}
