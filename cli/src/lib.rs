use std::error::Error;
use std::io::{self, BufRead, Write};
#[cfg(test)]
use std::path::PathBuf;

use tonic::transport::Endpoint;

mod args;
mod commands;
mod config;
mod output;
mod rpc;
pub mod tui;

pub use args::{
    AdminArgs, AdminCommand, BackupArgs, BackupCommand, Cli, Command, ConfigArgs, ConfigCommand,
    ConfigKey, GameArgs, GameCommand, HealthArgs, JobStatusArg, MemoryArgs, MemoryCommand,
    MetricsArgs, MetricsCommand, PageArgs, PlayArgs, SessionArgs, SessionCommand, UserArgs,
    UserCommand, join_words,
};
use commands::{admin, backup, config, game, health, memory, metrics, play, session, user};
pub use config::{ClientConfig, normalize_addr};
#[cfg(test)]
use config::{DEFAULT_CONFIG_FILE, LEGACY_CONFIG_FILE, normalize_optional_model, read_prompt};
use config::{
    config_path, invalid_input, load_config_from_path, required_user_id, resolve_addr,
    resolve_user_id, save_config_to_path,
};
pub(crate) use output::serving_status_name;
#[cfg(test)]
use output::{admin_status_name, finish_reason_name, message_json, role_name};
pub(crate) use rpc::with_user;

pub type CliResult<T> = Result<T, Box<dyn Error + Send + Sync>>;

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
#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;
    use harpe_proto::pb::{self, Message};

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
            "--model",
            "gpt-5-mini",
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
                    model: Some("gpt-5-mini".to_owned()),
                    session_id: "session-1".to_owned(),
                    content: vec!["open".to_owned(), "the".to_owned(), "gate".to_owned()],
                }
            })
        );
    }

    #[test]
    fn normalizes_optional_model_for_requests() {
        assert_eq!(normalize_optional_model(None), "");
        assert_eq!(normalize_optional_model(Some("   ".to_owned())), "");
        assert_eq!(
            normalize_optional_model(Some(" gpt-5-mini ".to_owned())),
            "gpt-5-mini"
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
        let path = temp_test_path("client-config.toml");
        let config = ClientConfig {
            addr: Some("http://127.0.0.1:50051".to_owned()),
            user_id: Some("user-1".to_owned()),
            game_id: Some("game-1".to_owned()),
            session_id: Some("session-1".to_owned()),
        };

        save_config_to_path(&path, &config).unwrap();
        let loaded = load_config_from_path(&path).unwrap();
        let content = std::fs::read_to_string(&path).unwrap();

        assert_eq!(loaded, config);
        assert!(content.contains("addr = \"http://127.0.0.1:50051\""));
        assert_eq!(
            resolve_addr(None, &loaded).unwrap(),
            "http://127.0.0.1:50051"
        );
        assert_eq!(resolve_user_id(None, &loaded), Some("user-1"));
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn client_config_still_reads_and_writes_explicit_json_paths() {
        let path = temp_test_path("client-config.json");
        let config = ClientConfig {
            addr: Some("http://127.0.0.1:50051".to_owned()),
            user_id: Some("user-1".to_owned()),
            game_id: None,
            session_id: None,
        };

        save_config_to_path(&path, &config).unwrap();
        let loaded = load_config_from_path(&path).unwrap();
        let content = std::fs::read_to_string(&path).unwrap();

        assert_eq!(loaded, config);
        assert!(content.contains("\"user_id\": \"user-1\""));
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn client_config_falls_back_to_legacy_json_when_default_toml_is_missing() {
        let dir = temp_test_path("legacy-config-dir");
        std::fs::create_dir_all(&dir).unwrap();
        let toml_path = dir.join(DEFAULT_CONFIG_FILE);
        let json_path = dir.join(LEGACY_CONFIG_FILE);
        std::fs::write(
            &json_path,
            r#"{
  "addr": "http://127.0.0.1:50051",
  "user_id": "user-1"
}"#,
        )
        .unwrap();

        let loaded = load_config_from_path(&toml_path).unwrap();

        assert_eq!(loaded.addr.as_deref(), Some("http://127.0.0.1:50051"));
        assert_eq!(loaded.user_id.as_deref(), Some("user-1"));
        std::fs::remove_file(json_path).unwrap();
        std::fs::remove_dir(dir).unwrap();
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
