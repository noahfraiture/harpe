use std::io::{self, Stdout};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use clap::Parser;
use crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event as TerminalEvent, KeyCode, KeyEvent,
    KeyModifiers,
};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use tokio::sync::mpsc;
use tonic::transport::Endpoint;
use tui_textarea::{Input, Key};

use crate::{
    CliResult, ClientConfig, config_path, load_config_from_path, normalize_addr, required_user_id,
    save_config_to_path,
};

const DEFAULT_ADDR: &str = "http://[::1]:50051";
const TICK_RATE: Duration = Duration::from_millis(80);
const HEALTH_INTERVAL: Duration = Duration::from_secs(8);
mod client;
mod render;
mod state;
mod text;

use client::TuiClient;
use render::render;
use state::{App, AppEvent, FinderMode, FinderState, RightTab};
#[cfg(test)]
use text::first_sentences;

#[derive(Parser, Debug, Clone, PartialEq, Eq)]
#[command(name = "harpe-tui")]
#[command(about = "Terminal roleplay cockpit for the Harpe backend")]
pub struct TuiArgs {
    #[arg(long, env = "HARPE_GRPC_ADDR")]
    pub addr: Option<String>,
    #[arg(long, env = "HARPE_USER_ID")]
    pub user_id: Option<String>,
    #[arg(long, env = "HARPE_CONFIG")]
    pub config: Option<PathBuf>,
    #[arg(long)]
    pub game_id: Option<String>,
    #[arg(long)]
    pub session_id: Option<String>,
    #[arg(long)]
    pub model: Option<String>,
}

pub async fn run(args: TuiArgs) -> CliResult<()> {
    let config_path = config_path(args.config.as_deref())?;
    let mut client_config = load_config_from_path(&config_path)?;
    let addr = resolve_tui_addr(args.addr.as_deref(), &client_config)?;
    let user_id = required_user_id(args.user_id.as_deref().or(client_config.user_id.as_deref()))?;
    let channel = Endpoint::from_shared(addr.clone())?.connect().await?;
    let client = TuiClient::new(channel, user_id.clone());

    let mut app = App::new(addr, user_id, args.model);
    let selected_game = args
        .game_id
        .as_deref()
        .or(client_config.game_id.as_deref())
        .map(ToOwned::to_owned);
    let selected_session = args
        .session_id
        .as_deref()
        .or(client_config.session_id.as_deref())
        .map(ToOwned::to_owned);

    if let Some(session_id) = selected_session {
        if let Err(error) = load_session(
            &client,
            &mut app,
            &mut client_config,
            &config_path,
            session_id,
        )
        .await
        {
            app.set_error(format!("failed to load session: {error}"));
            open_game_finder(&client, &mut app).await?;
        }
    } else if let Some(game_id) = selected_game {
        if let Err(error) =
            load_game(&client, &mut app, &mut client_config, &config_path, game_id).await
        {
            app.set_error(format!("failed to load game: {error}"));
            open_game_finder(&client, &mut app).await?;
        } else {
            open_session_finder(&client, &mut app).await?;
        }
    } else {
        open_game_finder(&client, &mut app).await?;
    }

    app.health = client.health().await.ok();

    let mut terminal = TerminalGuard::enter()?;
    let (tx, mut rx) = mpsc::channel::<AppEvent>(128);
    let mut last_health = Instant::now();

    loop {
        terminal.draw(|frame| render(frame, &mut app))?;

        while let Ok(event) = rx.try_recv() {
            handle_app_event(&mut app, event);
        }

        if app.quit {
            break;
        }

        if event::poll(TICK_RATE)?
            && let TerminalEvent::Key(key) = event::read()?
        {
            handle_key(
                key,
                &client,
                &mut app,
                &mut client_config,
                &config_path,
                tx.clone(),
            )
            .await?;
        }

        if last_health.elapsed() >= HEALTH_INTERVAL {
            if let Ok(health) = client.health().await {
                app.health = Some(health);
            }
            last_health = Instant::now();
        }
    }

    Ok(())
}

fn resolve_tui_addr(cli_addr: Option<&str>, config: &ClientConfig) -> CliResult<String> {
    normalize_addr(cli_addr.or(config.addr.as_deref()).unwrap_or(DEFAULT_ADDR))
}

struct TerminalGuard {
    terminal: Terminal<CrosstermBackend<Stdout>>,
}

impl TerminalGuard {
    fn enter() -> io::Result<Self> {
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
        let backend = CrosstermBackend::new(stdout);
        let terminal = Terminal::new(backend)?;
        Ok(Self { terminal })
    }

    fn draw<F>(&mut self, f: F) -> io::Result<()>
    where
        F: FnOnce(&mut ratatui::Frame<'_>),
    {
        self.terminal.draw(f).map(|_| ())
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(
            self.terminal.backend_mut(),
            LeaveAlternateScreen,
            DisableMouseCapture
        );
        let _ = self.terminal.show_cursor();
    }
}

async fn handle_key(
    key: KeyEvent,
    client: &TuiClient,
    app: &mut App,
    config: &mut ClientConfig,
    config_path: &Path,
    tx: mpsc::Sender<AppEvent>,
) -> CliResult<()> {
    if is_quit_key(key) {
        app.quit = true;
        return Ok(());
    }

    if app.help_open {
        match key.code {
            KeyCode::Esc | KeyCode::Char('?') => app.help_open = false,
            _ => {}
        }
        return Ok(());
    }

    if app.error.is_some() && dismisses_error(key) {
        app.error = None;
        return Ok(());
    }

    if app.finder.is_some() {
        return handle_finder_key(key, client, app, config, config_path).await;
    }

    match key {
        KeyEvent {
            code: KeyCode::Char('?'),
            modifiers: KeyModifiers::NONE,
            ..
        } => app.help_open = true,
        KeyEvent {
            code: KeyCode::Char('g'),
            modifiers: KeyModifiers::CONTROL,
            ..
        } => open_game_finder(client, app).await?,
        KeyEvent {
            code: KeyCode::Char('l'),
            modifiers: KeyModifiers::CONTROL,
            ..
        } => open_session_finder(client, app).await?,
        KeyEvent {
            code: KeyCode::Char('r'),
            modifiers: KeyModifiers::CONTROL,
            ..
        } => refresh_active_data(client, app).await?,
        KeyEvent {
            code: KeyCode::Char('t'),
            modifiers: KeyModifiers::CONTROL,
            ..
        } => app.right_tab = app.right_tab.next(),
        KeyEvent {
            code: KeyCode::Char('p'),
            modifiers: KeyModifiers::CONTROL,
            ..
        } => preview_context(client, app).await?,
        key if is_memory_search_key(key) => search_memory(client, app).await?,
        KeyEvent {
            code: KeyCode::PageUp,
            ..
        } => app.transcript_scroll = app.transcript_scroll.saturating_sub(5),
        KeyEvent {
            code: KeyCode::PageDown,
            ..
        } => app.transcript_scroll = app.transcript_scroll.saturating_add(5),
        KeyEvent {
            code: KeyCode::Enter,
            modifiers: KeyModifiers::ALT,
            ..
        }
        | KeyEvent {
            code: KeyCode::Char('j'),
            modifiers: KeyModifiers::CONTROL,
            ..
        } => {
            app.composer.input(Input {
                key: Key::Enter,
                ctrl: false,
                alt: false,
                shift: false,
            });
        }
        KeyEvent {
            code: KeyCode::Enter,
            ..
        } => send_current_message(client, app, tx).await?,
        _ => {
            app.composer.input(key);
        }
    }

    Ok(())
}

fn is_quit_key(key: KeyEvent) -> bool {
    matches!(
        key,
        KeyEvent {
            code: KeyCode::Char('c' | 'q'),
            modifiers: KeyModifiers::CONTROL,
            ..
        }
    )
}

fn dismisses_error(key: KeyEvent) -> bool {
    matches!(
        key,
        KeyEvent {
            code: KeyCode::Esc | KeyCode::Enter,
            ..
        } | KeyEvent {
            code: KeyCode::Char(_),
            modifiers: KeyModifiers::NONE,
            ..
        }
    )
}

fn is_memory_search_key(key: KeyEvent) -> bool {
    matches!(
        key,
        KeyEvent {
            code: KeyCode::Char('f'),
            modifiers: KeyModifiers::CONTROL,
            ..
        }
    )
}

async fn handle_finder_key(
    key: KeyEvent,
    client: &TuiClient,
    app: &mut App,
    config: &mut ClientConfig,
    config_path: &Path,
) -> CliResult<()> {
    let Some(finder) = app.finder.as_mut() else {
        return Ok(());
    };

    match key.code {
        KeyCode::Esc => app.finder = None,
        KeyCode::Up => finder.move_up(),
        KeyCode::Down => finder.move_down(),
        KeyCode::Backspace => finder.pop_query(),
        KeyCode::Enter => match finder.mode.clone() {
            FinderMode::Game => {
                let selected = finder
                    .filtered_games()
                    .get(finder.selected)
                    .cloned()
                    .cloned();
                if let Some(game) = selected {
                    let game_id = game.id.clone();
                    app.game = Some(game);
                    config.game_id = Some(game_id.clone());
                    config.session_id = None;
                    save_config_to_path(config_path, config)?;
                    let sessions = client.list_sessions(game_id.clone()).await?;
                    app.finder = Some(FinderState::session(game_id, sessions));
                }
            }
            FinderMode::Session { .. } => {
                let selected = finder
                    .filtered_sessions()
                    .get(finder.selected)
                    .cloned()
                    .cloned();
                if let Some(session) = selected {
                    let session_id = session.id.clone();
                    app.finder = None;
                    load_session(client, app, config, config_path, session_id).await?;
                }
            }
        },
        KeyCode::Char(char) if key.modifiers.is_empty() => finder.push_query(char),
        _ => {}
    }
    Ok(())
}

async fn open_game_finder(client: &TuiClient, app: &mut App) -> CliResult<()> {
    app.finder = Some(FinderState::game(client.list_games().await?));
    app.set_status("select a game");
    Ok(())
}

async fn open_session_finder(client: &TuiClient, app: &mut App) -> CliResult<()> {
    let Some(game) = app.game.as_ref() else {
        return open_game_finder(client, app).await;
    };
    let sessions = client.list_sessions(game.id.clone()).await?;
    app.finder = Some(FinderState::session(game.id.clone(), sessions));
    app.set_status("select a session");
    Ok(())
}

async fn load_game(
    client: &TuiClient,
    app: &mut App,
    config: &mut ClientConfig,
    config_path: &Path,
    game_id: String,
) -> CliResult<()> {
    let game = client.get_game(game_id).await?;
    config.game_id = Some(game.id.clone());
    save_config_to_path(config_path, config)?;
    app.game = Some(game);
    Ok(())
}

async fn load_session(
    client: &TuiClient,
    app: &mut App,
    config: &mut ClientConfig,
    config_path: &Path,
    session_id: String,
) -> CliResult<()> {
    let session = client.get_session(session_id).await?;
    if app.game.as_ref().map(|game| game.id.as_str()) != Some(session.game_id.as_str()) {
        let game = client.get_game(session.game_id.clone()).await?;
        app.game = Some(game);
    }

    config.game_id = Some(session.game_id.clone());
    config.session_id = Some(session.id.clone());
    save_config_to_path(config_path, config)?;
    app.session = Some(session);
    refresh_active_data(client, app).await?;
    Ok(())
}

async fn refresh_active_data(client: &TuiClient, app: &mut App) -> CliResult<()> {
    let Some(session) = app.session.as_ref() else {
        app.set_status("no session selected");
        return Ok(());
    };
    let session_id = session.id.clone();
    let game_id = session.game_id.clone();

    app.messages = client
        .list_messages(session_id.clone())
        .await
        .unwrap_or_default();
    app.summary = client.summary(session_id.clone()).await.ok();
    app.characters = client.characters(game_id.clone()).await.unwrap_or_default();
    app.events = client.events(session_id).await.unwrap_or_default();
    app.facts = client.facts(game_id.clone()).await.unwrap_or_default();
    app.locations = client.locations(game_id).await.unwrap_or_default();
    app.health = client.health().await.ok();
    app.set_status("refreshed");
    Ok(())
}

async fn preview_context(client: &TuiClient, app: &mut App) -> CliResult<()> {
    let Some(session) = app.session.as_ref() else {
        app.set_status("select a session first");
        return Ok(());
    };
    let content = app.composer_content();
    let content = if content.is_empty() {
        "Continue the scene.".to_owned()
    } else {
        content
    };
    app.context_preview = Some(client.preview_context(session.id.clone(), content).await?);
    app.right_tab = RightTab::Context;
    app.set_status("context preview refreshed");
    Ok(())
}

async fn search_memory(client: &TuiClient, app: &mut App) -> CliResult<()> {
    let Some(session) = app.session.as_ref() else {
        app.set_status("select a session first");
        return Ok(());
    };
    let query = app.composer_content();
    if query.is_empty() {
        app.set_status("type a query in the composer before Ctrl-F");
        return Ok(());
    }
    app.search_results = client.search_memory(session.id.clone(), query).await?;
    app.right_tab = RightTab::Context;
    app.set_status("memory search complete");
    Ok(())
}

async fn send_current_message(
    client: &TuiClient,
    app: &mut App,
    tx: mpsc::Sender<AppEvent>,
) -> CliResult<()> {
    if app.streaming {
        app.set_status("assistant is still responding");
        return Ok(());
    }
    let Some(session) = app.session.as_ref() else {
        app.set_status("select a session first");
        return Ok(());
    };
    let session_id = session.id.clone();
    let content = app.composer_content();
    if content.is_empty() {
        return Ok(());
    }

    app.push_user_message(content.clone());
    app.start_assistant_message();
    app.clear_composer();
    app.streaming = true;
    app.set_status("streaming assistant response");

    let client = client.clone();
    let model = app.model.clone();
    tokio::spawn(async move {
        client.stream_message(session_id, content, model, tx).await;
    });
    Ok(())
}

fn handle_app_event(app: &mut App, event: AppEvent) {
    match event {
        AppEvent::AssistantDelta(delta) => app.append_assistant_delta(&delta),
        AppEvent::SendComplete => {
            app.streaming = false;
            app.set_status("assistant complete; memory update queued");
        }
        AppEvent::SendFailed(error) => {
            app.streaming = false;
            app.set_error(error);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use harpe_proto::pb;
    use harpe_proto::pb::{Game, Message, Session};
    use ratatui::backend::TestBackend;

    #[test]
    fn right_tabs_cycle_in_stable_order() {
        assert_eq!(RightTab::Cast.next(), RightTab::Lore);
        assert_eq!(RightTab::Context.next(), RightTab::Cast);
    }

    #[test]
    fn tui_args_parse_runtime_selection_options() {
        let args = TuiArgs::try_parse_from([
            "harpe-tui",
            "--addr",
            "http://harpe:50051",
            "--user-id",
            "user-1",
            "--game-id",
            "game-1",
            "--session-id",
            "session-1",
            "--model",
            "gpt-5-nano",
        ])
        .unwrap();

        assert_eq!(args.addr.as_deref(), Some("http://harpe:50051"));
        assert_eq!(args.user_id.as_deref(), Some("user-1"));
        assert_eq!(args.game_id.as_deref(), Some("game-1"));
        assert_eq!(args.session_id.as_deref(), Some("session-1"));
        assert_eq!(args.model.as_deref(), Some("gpt-5-nano"));
    }

    #[test]
    fn finder_filters_case_insensitively_and_wraps_selection() {
        let mut finder = FinderState::game(vec![
            Game {
                id: "game-1".to_owned(),
                title: "Black Gate".to_owned(),
                system_prompt: String::new(),
                created_at: String::new(),
                owner_user_id: "user-1".to_owned(),
            },
            Game {
                id: "game-2".to_owned(),
                title: "Old Harbor".to_owned(),
                system_prompt: String::new(),
                created_at: String::new(),
                owner_user_id: "user-1".to_owned(),
            },
        ]);

        finder.push_query('g');
        assert_eq!(finder.filtered_games()[0].id, "game-1");
        finder.move_down();
        assert_eq!(finder.selected, 0);
    }

    #[test]
    fn finder_filters_sessions_and_keeps_selection_in_range_after_backspace() {
        let mut finder = FinderState::session(
            "game-1".to_owned(),
            vec![
                Session {
                    id: "session-1".to_owned(),
                    game_id: "game-1".to_owned(),
                    title: "First Watch".to_owned(),
                    created_at: String::new(),
                },
                Session {
                    id: "session-2".to_owned(),
                    game_id: "game-1".to_owned(),
                    title: "Second Watch".to_owned(),
                    created_at: String::new(),
                },
            ],
        );

        finder.push_query('s');
        finder.push_query('e');
        assert_eq!(finder.filtered_sessions()[0].id, "session-2");
        finder.move_down();
        finder.pop_query();

        assert_eq!(finder.selected, 0);
        assert_eq!(finder.filtered_sessions().len(), 2);
    }

    #[test]
    fn app_composer_content_trims_multiline_text() {
        let mut app = App::new(
            "http://127.0.0.1:50051".to_owned(),
            "user-1".to_owned(),
            None,
        );
        app.composer.insert_str("  first line\nsecond line  ");

        assert_eq!(app.composer_content(), "first line\nsecond line");
    }

    #[test]
    fn app_message_lifecycle_records_pending_user_and_streamed_assistant_text() {
        let mut app = App::new(
            "http://127.0.0.1:50051".to_owned(),
            "user-1".to_owned(),
            None,
        );
        app.session = Some(Session {
            id: "session-1".to_owned(),
            game_id: "game-1".to_owned(),
            title: "First watch".to_owned(),
            created_at: String::new(),
        });

        app.push_user_message("I open the gate.".to_owned());
        app.start_assistant_message();
        app.append_assistant_delta("The gate ");
        app.append_assistant_delta("opens.");

        assert_eq!(app.messages.len(), 2);
        assert_eq!(app.messages[0].session_id, "session-1");
        assert_eq!(app.messages[0].role, pb::MessageRole::User as i32);
        assert_eq!(app.messages[1].role, pb::MessageRole::Assistant as i32);
        assert_eq!(app.messages[1].content, "The gate opens.");
    }

    #[test]
    fn app_events_update_streaming_status_and_error_state() {
        let mut app = App::new(
            "http://127.0.0.1:50051".to_owned(),
            "user-1".to_owned(),
            None,
        );
        app.streaming = true;
        app.start_assistant_message();

        handle_app_event(
            &mut app,
            AppEvent::AssistantDelta("A bell rings.".to_owned()),
        );
        assert_eq!(app.messages[0].content, "A bell rings.");

        handle_app_event(&mut app, AppEvent::SendComplete);
        assert!(!app.streaming);
        assert!(app.error.is_none());
        assert_eq!(app.status, "assistant complete; memory update queued");

        app.streaming = true;
        handle_app_event(&mut app, AppEvent::SendFailed("network down".to_owned()));
        assert!(!app.streaming);
        assert_eq!(app.error.as_deref(), Some("network down"));
    }

    #[test]
    fn error_dismiss_and_quit_key_helpers_match_tui_contract() {
        assert!(is_quit_key(KeyEvent::new(
            KeyCode::Char('q'),
            KeyModifiers::CONTROL
        )));
        assert!(is_quit_key(KeyEvent::new(
            KeyCode::Char('c'),
            KeyModifiers::CONTROL
        )));
        assert!(!is_quit_key(KeyEvent::new(
            KeyCode::Char('q'),
            KeyModifiers::NONE
        )));

        assert!(dismisses_error(KeyEvent::new(
            KeyCode::Esc,
            KeyModifiers::NONE
        )));
        assert!(dismisses_error(KeyEvent::new(
            KeyCode::Enter,
            KeyModifiers::NONE
        )));
        assert!(dismisses_error(KeyEvent::new(
            KeyCode::Char('x'),
            KeyModifiers::NONE
        )));
        assert!(!dismisses_error(KeyEvent::new(
            KeyCode::Char('x'),
            KeyModifiers::CONTROL
        )));
    }

    #[test]
    fn memory_search_uses_terminal_safe_ctrl_f_binding() {
        assert!(is_memory_search_key(KeyEvent::new(
            KeyCode::Char('f'),
            KeyModifiers::CONTROL
        )));
        assert!(!is_memory_search_key(KeyEvent::new(
            KeyCode::Char('m'),
            KeyModifiers::CONTROL
        )));
        assert!(!is_memory_search_key(KeyEvent::new(
            KeyCode::Enter,
            KeyModifiers::NONE
        )));
    }

    #[test]
    fn first_sentences_prefers_complete_sentence_boundary() {
        assert_eq!(
            first_sentences("The gate opens. A bell rings. The floor shakes.", 2),
            "The gate opens. A bell rings."
        );
    }

    #[test]
    fn first_sentences_truncates_when_no_sentence_boundary_exists() {
        let content = "a".repeat(260);

        assert_eq!(first_sentences(&content, 2).chars().count(), 220);
        assert!(first_sentences(&content, 2).ends_with("..."));
    }

    #[test]
    fn render_main_layout_does_not_panic() {
        let backend = TestBackend::new(130, 42);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = App::new(
            "http://127.0.0.1:50051".to_owned(),
            "user-1".to_owned(),
            Some("gpt-5-nano".to_owned()),
        );
        app.game = Some(Game {
            id: "game-1".to_owned(),
            title: "Ashes".to_owned(),
            system_prompt: String::new(),
            created_at: String::new(),
            owner_user_id: "user-1".to_owned(),
        });
        app.session = Some(Session {
            id: "session-1".to_owned(),
            game_id: "game-1".to_owned(),
            title: "Black Gate".to_owned(),
            created_at: String::new(),
        });
        app.messages.push(Message {
            id: "message-1".to_owned(),
            session_id: "session-1".to_owned(),
            role: pb::MessageRole::User as i32,
            content: "I listen at the gate.".to_owned(),
            created_at: String::new(),
        });

        terminal.draw(|frame| render(frame, &mut app)).unwrap();
    }

    #[test]
    fn render_compact_layout_with_overlays_does_not_panic() {
        let backend = TestBackend::new(76, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = App::new(
            "http://127.0.0.1:50051".to_owned(),
            "user-1".to_owned(),
            None,
        );
        app.finder = Some(FinderState::game(vec![Game {
            id: "game-1".to_owned(),
            title: "Ashes".to_owned(),
            system_prompt: String::new(),
            created_at: String::new(),
            owner_user_id: "user-1".to_owned(),
        }]));
        app.help_open = true;
        app.error = Some("network down".to_owned());

        terminal.draw(|frame| render(frame, &mut app)).unwrap();
    }
}
