use std::path::PathBuf;

use clap::{Args, Parser, Subcommand, ValueEnum};
use harpe_proto::pb::{AdminJobStatus, PageRequest};

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
    #[arg(long)]
    pub model: Option<String>,
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
        #[arg(long)]
        model: Option<String>,
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
    pub(crate) fn request(&self) -> PageRequest {
        PageRequest {
            page_size: self.limit,
            page_token: self.page_token.clone().unwrap_or_default(),
        }
    }
}

pub fn join_words(words: Vec<String>) -> String {
    words.join(" ")
}

pub(crate) fn admin_status_filter(status: JobStatusArg) -> i32 {
    match status {
        JobStatusArg::All => AdminJobStatus::Unspecified as i32,
        JobStatusArg::Pending => AdminJobStatus::Pending as i32,
        JobStatusArg::Running => AdminJobStatus::Running as i32,
        JobStatusArg::Succeeded => AdminJobStatus::Succeeded as i32,
        JobStatusArg::Failed => AdminJobStatus::Failed as i32,
    }
}
