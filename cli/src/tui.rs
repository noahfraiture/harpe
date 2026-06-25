use std::cmp::min;
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
use futures_util::StreamExt;
use harpe_server::pb::{
    self, Character, ContextMessage, Event, Game, GetGameRequest, GetSessionRequest,
    GetStorySummaryRequest, HealthCheckRequest, HealthCheckResponse, ListCharactersRequest,
    ListEventsRequest, ListGamesRequest, ListLocationsRequest, ListMessagesRequest,
    ListSessionsRequest, ListWorldFactsRequest, Location, Message, PageRequest,
    PreviewContextRequest, SearchMemoryRequest, SendMessageRequest, Session, StorySummary,
    WorldFact, game_service_client::GameServiceClient, health_service_client::HealthServiceClient,
    memory_service_client::MemoryServiceClient, session_service_client::SessionServiceClient,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, Borders, Clear, List, ListItem, Paragraph, Scrollbar, ScrollbarOrientation,
    ScrollbarState, Tabs, Wrap,
};
use tokio::sync::mpsc;
use tonic::transport::{Channel, Endpoint};
use tui_textarea::{Input, Key, TextArea};

use crate::{
    CliResult, ClientConfig, config_path, load_config_from_path, normalize_addr, required_user_id,
    save_config_to_path, serving_status_name, with_user,
};

const DEFAULT_ADDR: &str = "http://[::1]:50051";
const TICK_RATE: Duration = Duration::from_millis(80);
const HEALTH_INTERVAL: Duration = Duration::from_secs(8);
const DEFAULT_PAGE_SIZE: u32 = 50;

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

#[derive(Clone)]
struct TuiClient {
    channel: Channel,
    user_id: String,
}

impl TuiClient {
    fn new(channel: Channel, user_id: String) -> Self {
        Self { channel, user_id }
    }

    async fn health(&self) -> CliResult<HealthCheckResponse> {
        Ok(HealthServiceClient::new(self.channel.clone())
            .check(HealthCheckRequest {
                service: String::new(),
            })
            .await?
            .into_inner())
    }

    async fn list_games(&self) -> CliResult<Vec<Game>> {
        Ok(GameServiceClient::new(self.channel.clone())
            .list_games(with_user(
                ListGamesRequest {
                    limit: 0,
                    page: Some(page(DEFAULT_PAGE_SIZE)),
                },
                &self.user_id,
            )?)
            .await?
            .into_inner()
            .games)
    }

    async fn get_game(&self, game_id: String) -> CliResult<Game> {
        Ok(GameServiceClient::new(self.channel.clone())
            .get_game(with_user(GetGameRequest { game_id }, &self.user_id)?)
            .await?
            .into_inner())
    }

    async fn list_sessions(&self, game_id: String) -> CliResult<Vec<Session>> {
        Ok(SessionServiceClient::new(self.channel.clone())
            .list_sessions(with_user(
                ListSessionsRequest {
                    game_id,
                    limit: 0,
                    page: Some(page(DEFAULT_PAGE_SIZE)),
                },
                &self.user_id,
            )?)
            .await?
            .into_inner()
            .sessions)
    }

    async fn get_session(&self, session_id: String) -> CliResult<Session> {
        Ok(SessionServiceClient::new(self.channel.clone())
            .get_session(with_user(GetSessionRequest { session_id }, &self.user_id)?)
            .await?
            .into_inner())
    }

    async fn list_messages(&self, session_id: String) -> CliResult<Vec<Message>> {
        Ok(SessionServiceClient::new(self.channel.clone())
            .list_messages(with_user(
                ListMessagesRequest {
                    session_id,
                    limit: 0,
                    page: Some(page(80)),
                },
                &self.user_id,
            )?)
            .await?
            .into_inner()
            .messages)
    }

    async fn summary(&self, session_id: String) -> CliResult<StorySummary> {
        Ok(MemoryServiceClient::new(self.channel.clone())
            .get_story_summary(with_user(
                GetStorySummaryRequest { session_id },
                &self.user_id,
            )?)
            .await?
            .into_inner())
    }

    async fn characters(&self, game_id: String) -> CliResult<Vec<Character>> {
        Ok(MemoryServiceClient::new(self.channel.clone())
            .list_characters(with_user(
                ListCharactersRequest {
                    game_id,
                    limit: 0,
                    page: Some(page(30)),
                },
                &self.user_id,
            )?)
            .await?
            .into_inner()
            .characters)
    }

    async fn events(&self, session_id: String) -> CliResult<Vec<Event>> {
        Ok(MemoryServiceClient::new(self.channel.clone())
            .list_events(with_user(
                ListEventsRequest {
                    session_id,
                    limit: 0,
                    page: Some(page(30)),
                },
                &self.user_id,
            )?)
            .await?
            .into_inner()
            .events)
    }

    async fn facts(&self, game_id: String) -> CliResult<Vec<WorldFact>> {
        Ok(MemoryServiceClient::new(self.channel.clone())
            .list_world_facts(with_user(
                ListWorldFactsRequest {
                    game_id,
                    limit: 0,
                    page: Some(page(30)),
                },
                &self.user_id,
            )?)
            .await?
            .into_inner()
            .facts)
    }

    async fn locations(&self, game_id: String) -> CliResult<Vec<Location>> {
        Ok(MemoryServiceClient::new(self.channel.clone())
            .list_locations(with_user(
                ListLocationsRequest {
                    game_id,
                    limit: 0,
                    page: Some(page(20)),
                },
                &self.user_id,
            )?)
            .await?
            .into_inner()
            .locations)
    }

    async fn preview_context(
        &self,
        session_id: String,
        content: String,
    ) -> CliResult<ContextPreview> {
        let response = SessionServiceClient::new(self.channel.clone())
            .preview_context(with_user(
                PreviewContextRequest {
                    session_id,
                    content,
                },
                &self.user_id,
            )?)
            .await?
            .into_inner();
        Ok(ContextPreview {
            estimated_tokens: response.estimated_tokens,
            messages: response.messages,
        })
    }

    async fn search_memory(&self, session_id: String, query: String) -> CliResult<Vec<String>> {
        let hits = MemoryServiceClient::new(self.channel.clone())
            .search_memory(with_user(
                SearchMemoryRequest {
                    session_id,
                    query,
                    limit: 10,
                    page: None,
                },
                &self.user_id,
            )?)
            .await?
            .into_inner()
            .hits;
        Ok(hits
            .into_iter()
            .map(|hit| format!("{} [{:.2}] {}", hit.kind, hit.score, hit.content))
            .collect())
    }

    async fn stream_message(
        &self,
        session_id: String,
        content: String,
        model: Option<String>,
        tx: mpsc::Sender<AppEvent>,
    ) {
        let result = self
            .stream_message_inner(session_id, content, model, tx.clone())
            .await;
        if let Err(error) = result {
            let _ = tx.send(AppEvent::SendFailed(error.to_string())).await;
        }
    }

    async fn stream_message_inner(
        &self,
        session_id: String,
        content: String,
        model: Option<String>,
        tx: mpsc::Sender<AppEvent>,
    ) -> CliResult<()> {
        let mut stream = SessionServiceClient::new(self.channel.clone())
            .send_message(with_user(
                SendMessageRequest {
                    session_id,
                    content,
                    model: normalize_model(model),
                },
                &self.user_id,
            )?)
            .await?
            .into_inner();

        while let Some(next) = stream.next().await {
            let delta = next?;
            if !delta.delta.is_empty() {
                tx.send(AppEvent::AssistantDelta(delta.delta)).await?;
            }
            if delta.done {
                tx.send(AppEvent::SendComplete).await?;
                break;
            }
        }
        Ok(())
    }
}

fn page(page_size: u32) -> PageRequest {
    PageRequest {
        page_size,
        page_token: String::new(),
    }
}

fn normalize_model(model: Option<String>) -> String {
    model
        .map(|model| model.trim().to_owned())
        .filter(|model| !model.is_empty())
        .unwrap_or_default()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RightTab {
    Cast,
    Lore,
    Map,
    Events,
    Context,
}

impl RightTab {
    const ALL: [Self; 5] = [
        Self::Cast,
        Self::Lore,
        Self::Map,
        Self::Events,
        Self::Context,
    ];

    fn title(self) -> &'static str {
        match self {
            Self::Cast => "Cast",
            Self::Lore => "Lore",
            Self::Map => "Map",
            Self::Events => "Events",
            Self::Context => "Context",
        }
    }

    fn next(self) -> Self {
        let index = Self::ALL.iter().position(|tab| *tab == self).unwrap_or(0);
        Self::ALL[(index + 1) % Self::ALL.len()]
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum FinderMode {
    Game,
    Session { game_id: String },
}

#[derive(Debug, Clone)]
struct FinderState {
    mode: FinderMode,
    query: String,
    selected: usize,
    games: Vec<Game>,
    sessions: Vec<Session>,
}

impl FinderState {
    fn game(games: Vec<Game>) -> Self {
        Self {
            mode: FinderMode::Game,
            query: String::new(),
            selected: 0,
            games,
            sessions: Vec::new(),
        }
    }

    fn session(game_id: String, sessions: Vec<Session>) -> Self {
        Self {
            mode: FinderMode::Session { game_id },
            query: String::new(),
            selected: 0,
            games: Vec::new(),
            sessions,
        }
    }

    fn title(&self) -> &'static str {
        match self.mode {
            FinderMode::Game => "Find Game",
            FinderMode::Session { .. } => "Find Session",
        }
    }

    fn move_down(&mut self) {
        let len = self.filtered_len();
        if len > 0 {
            self.selected = (self.selected + 1) % len;
        }
    }

    fn move_up(&mut self) {
        let len = self.filtered_len();
        if len > 0 {
            self.selected = if self.selected == 0 {
                len - 1
            } else {
                self.selected - 1
            };
        }
    }

    fn push_query(&mut self, char: char) {
        self.query.push(char);
        self.selected = 0;
    }

    fn pop_query(&mut self) {
        self.query.pop();
        self.selected = min(self.selected, self.filtered_len().saturating_sub(1));
    }

    fn filtered_len(&self) -> usize {
        match self.mode {
            FinderMode::Game => self.filtered_games().len(),
            FinderMode::Session { .. } => self.filtered_sessions().len(),
        }
    }

    fn filtered_games(&self) -> Vec<&Game> {
        let query = self.query.to_lowercase();
        self.games
            .iter()
            .filter(|game| query.is_empty() || game.title.to_lowercase().contains(&query))
            .collect()
    }

    fn filtered_sessions(&self) -> Vec<&Session> {
        let query = self.query.to_lowercase();
        self.sessions
            .iter()
            .filter(|session| query.is_empty() || session.title.to_lowercase().contains(&query))
            .collect()
    }
}

#[derive(Debug, Clone)]
struct ContextPreview {
    estimated_tokens: u32,
    messages: Vec<ContextMessage>,
}

struct App {
    addr: String,
    user_id: String,
    model: Option<String>,
    game: Option<Game>,
    session: Option<Session>,
    messages: Vec<Message>,
    summary: Option<StorySummary>,
    characters: Vec<Character>,
    events: Vec<Event>,
    facts: Vec<WorldFact>,
    locations: Vec<Location>,
    context_preview: Option<ContextPreview>,
    health: Option<HealthCheckResponse>,
    composer: TextArea<'static>,
    right_tab: RightTab,
    finder: Option<FinderState>,
    help_open: bool,
    search_results: Vec<String>,
    transcript_scroll: u16,
    status: String,
    error: Option<String>,
    streaming: bool,
    quit: bool,
}

impl App {
    fn new(addr: String, user_id: String, model: Option<String>) -> Self {
        let mut composer = TextArea::default();
        composer.set_block(
            Block::default()
                .borders(Borders::ALL)
                .title("Composer")
                .border_style(Style::default().fg(Color::DarkGray)),
        );
        composer.set_cursor_line_style(Style::default());
        Self {
            addr,
            user_id,
            model,
            game: None,
            session: None,
            messages: Vec::new(),
            summary: None,
            characters: Vec::new(),
            events: Vec::new(),
            facts: Vec::new(),
            locations: Vec::new(),
            context_preview: None,
            health: None,
            composer,
            right_tab: RightTab::Cast,
            finder: None,
            help_open: false,
            search_results: Vec::new(),
            transcript_scroll: 0,
            status: "ready".to_owned(),
            error: None,
            streaming: false,
            quit: false,
        }
    }

    fn set_error(&mut self, error: impl Into<String>) {
        self.error = Some(error.into());
    }

    fn set_status(&mut self, status: impl Into<String>) {
        self.status = status.into();
        self.error = None;
    }

    fn composer_content(&self) -> String {
        self.composer.lines().join("\n").trim().to_owned()
    }

    fn clear_composer(&mut self) {
        self.composer = TextArea::default();
        self.composer.set_block(
            Block::default()
                .borders(Borders::ALL)
                .title("Composer")
                .border_style(Style::default().fg(Color::DarkGray)),
        );
        self.composer.set_cursor_line_style(Style::default());
    }

    fn push_user_message(&mut self, content: String) {
        self.messages.push(Message {
            id: String::new(),
            session_id: self
                .session
                .as_ref()
                .map(|session| session.id.clone())
                .unwrap_or_default(),
            role: pb::MessageRole::User as i32,
            content,
            created_at: "pending".to_owned(),
        });
    }

    fn start_assistant_message(&mut self) {
        self.messages.push(Message {
            id: String::new(),
            session_id: self
                .session
                .as_ref()
                .map(|session| session.id.clone())
                .unwrap_or_default(),
            role: pb::MessageRole::Assistant as i32,
            content: String::new(),
            created_at: "streaming".to_owned(),
        });
    }

    fn append_assistant_delta(&mut self, delta: &str) {
        if let Some(message) = self
            .messages
            .iter_mut()
            .rev()
            .find(|message| message.role == pb::MessageRole::Assistant as i32)
        {
            message.content.push_str(delta);
        }
    }
}

enum AppEvent {
    AssistantDelta(String),
    SendComplete,
    SendFailed(String),
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

fn render(frame: &mut ratatui::Frame<'_>, app: &mut App) {
    let area = frame.area();
    let root = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),
            Constraint::Min(12),
            Constraint::Length(6),
            Constraint::Length(1),
        ])
        .split(area);

    render_header(frame, app, root[0]);
    render_body(frame, app, root[1]);
    render_composer(frame, app, root[2]);
    render_footer(frame, app, root[3]);

    if app.help_open {
        render_help(frame, area);
    }
    if app.finder.is_some() {
        render_finder(frame, app, area);
    }
    if let Some(error) = app.error.as_ref() {
        render_error(frame, error, area);
    }
}

fn render_header(frame: &mut ratatui::Frame<'_>, app: &App, area: Rect) {
    let game = app
        .game
        .as_ref()
        .map(|game| game.title.as_str())
        .unwrap_or("No game");
    let session = app
        .session
        .as_ref()
        .map(|session| session.title.as_str())
        .unwrap_or("No session");
    let model = app.model.as_deref().unwrap_or("server default");
    let health = app
        .health
        .as_ref()
        .map(|health| {
            format!(
                "{} p:{} f:{}",
                serving_status_name(health.status),
                health.pending_jobs,
                health.failed_jobs
            )
        })
        .unwrap_or_else(|| "health unknown".to_owned());
    let title = format!("Harpe - {game} / {session}");
    let meta = format!(
        "user: {} | model: {model} | {health} | {}",
        app.user_id, app.status
    );

    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
        .split(area);
    frame.render_widget(
        Paragraph::new(title).style(Style::default().fg(Color::Cyan)),
        chunks[0],
    );
    frame.render_widget(
        Paragraph::new(meta)
            .alignment(Alignment::Right)
            .style(Style::default().fg(Color::Gray)),
        chunks[1],
    );
}

fn render_body(frame: &mut ratatui::Frame<'_>, app: &App, area: Rect) {
    if area.width >= 116 {
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Length(31),
                Constraint::Min(42),
                Constraint::Length(34),
            ])
            .split(area);
        render_scene(frame, app, chunks[0]);
        render_transcript(frame, app, chunks[1]);
        render_right_panel(frame, app, chunks[2]);
    } else if area.width >= 82 {
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Min(45), Constraint::Length(32)])
            .split(area);
        render_transcript(frame, app, chunks[0]);
        render_right_panel(frame, app, chunks[1]);
    } else {
        render_transcript(frame, app, area);
    }
}

fn render_scene(frame: &mut ratatui::Frame<'_>, app: &App, area: Rect) {
    let mut lines = Vec::new();
    lines.push(Line::from(Span::styled(
        "Location",
        Style::default().fg(Color::Yellow),
    )));
    if let Some(location) = app.locations.first() {
        lines.push(Line::from(location.name.clone()));
        lines.push(Line::from(truncate(&location.description, 120)));
    } else {
        lines.push(Line::from("Unknown"));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "Situation",
        Style::default().fg(Color::Yellow),
    )));
    lines.push(Line::from(scene_summary(app)));
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "Open Threads",
        Style::default().fg(Color::Yellow),
    )));
    for event in app.events.iter().rev().take(4) {
        lines.push(Line::from(format!("- {}", truncate(&event.summary, 54))));
    }
    if app.events.is_empty() {
        lines.push(Line::from("- No major events yet"));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "Recent Context",
        Style::default().fg(Color::Yellow),
    )));
    for fact in app.facts.iter().take(3) {
        lines.push(Line::from(format!("- {}", truncate(&fact.content, 54))));
    }
    if app.facts.is_empty() {
        lines.push(Line::from("- No world facts yet"));
    }

    frame.render_widget(
        Paragraph::new(lines)
            .block(Block::default().borders(Borders::ALL).title("Scene"))
            .wrap(Wrap { trim: false }),
        area,
    );
}

fn scene_summary(app: &App) -> String {
    app.summary
        .as_ref()
        .map(|summary| first_sentences(&summary.content, 2))
        .filter(|summary| !summary.is_empty())
        .or_else(|| app.session.as_ref().map(|session| session.title.clone()))
        .unwrap_or_else(|| "Select a session to begin.".to_owned())
}

fn render_transcript(frame: &mut ratatui::Frame<'_>, app: &App, area: Rect) {
    let mut lines = Vec::new();
    if app.messages.is_empty() {
        lines.push(Line::from(Span::styled(
            "No messages yet. Type below and press Enter.",
            Style::default().fg(Color::DarkGray),
        )));
    }
    for message in &app.messages {
        let role = role_name(message.role);
        let role_style = match pb::MessageRole::try_from(message.role).ok() {
            Some(pb::MessageRole::User) => Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
            Some(pb::MessageRole::Assistant) => Style::default()
                .fg(Color::Blue)
                .add_modifier(Modifier::BOLD),
            Some(pb::MessageRole::System) => Style::default().fg(Color::Magenta),
            _ => Style::default().fg(Color::Gray),
        };
        lines.push(Line::from(Span::styled(role, role_style)));
        for line in wrap_owned(&message.content, area.width.saturating_sub(4) as usize) {
            lines.push(Line::from(line));
        }
        lines.push(Line::from(""));
    }

    let mut scrollbar = ScrollbarState::new(lines.len()).position(app.transcript_scroll as usize);
    frame.render_widget(
        Paragraph::new(lines)
            .block(Block::default().borders(Borders::ALL).title("Transcript"))
            .scroll((app.transcript_scroll, 0))
            .wrap(Wrap { trim: false }),
        area,
    );
    frame.render_stateful_widget(
        Scrollbar::default().orientation(ScrollbarOrientation::VerticalRight),
        area,
        &mut scrollbar,
    );
}

fn render_right_panel(frame: &mut ratatui::Frame<'_>, app: &App, area: Rect) {
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(4)])
        .split(area);
    let titles = RightTab::ALL
        .iter()
        .map(|tab| Line::from(tab.title()))
        .collect::<Vec<_>>();
    let selected = RightTab::ALL
        .iter()
        .position(|tab| *tab == app.right_tab)
        .unwrap_or(0);
    frame.render_widget(
        Tabs::new(titles)
            .select(selected)
            .block(Block::default().borders(Borders::ALL))
            .style(Style::default().fg(Color::Gray))
            .highlight_style(Style::default().fg(Color::Cyan)),
        vertical[0],
    );

    match app.right_tab {
        RightTab::Cast => render_cast(frame, app, vertical[1]),
        RightTab::Lore => render_lore(frame, app, vertical[1]),
        RightTab::Map => render_map(frame, app, vertical[1]),
        RightTab::Events => render_events(frame, app, vertical[1]),
        RightTab::Context => render_context(frame, app, vertical[1]),
    }
}

fn render_cast(frame: &mut ratatui::Frame<'_>, app: &App, area: Rect) {
    let items = if app.characters.is_empty() {
        vec![ListItem::new("No characters yet")]
    } else {
        app.characters
            .iter()
            .map(|character| {
                ListItem::new(vec![
                    Line::from(Span::styled(
                        character.name.clone(),
                        Style::default().fg(Color::Yellow),
                    )),
                    Line::from(format!(
                        "status: {}",
                        blank_as(&character.status, "unknown")
                    )),
                    Line::from(truncate(&character.description, 90)),
                ])
            })
            .collect()
    };
    frame.render_widget(
        List::new(items).block(Block::default().borders(Borders::ALL).title("Cast")),
        area,
    );
}

fn render_lore(frame: &mut ratatui::Frame<'_>, app: &App, area: Rect) {
    let lines = if app.facts.is_empty() {
        vec![Line::from("No world facts yet")]
    } else {
        app.facts
            .iter()
            .flat_map(|fact| {
                [
                    Line::from(Span::styled(
                        format!("{:.0}% confidence", fact.confidence * 100.0),
                        Style::default().fg(Color::DarkGray),
                    )),
                    Line::from(truncate(&fact.content, 120)),
                    Line::from(""),
                ]
            })
            .collect()
    };
    frame.render_widget(
        Paragraph::new(lines)
            .block(Block::default().borders(Borders::ALL).title("Lore"))
            .wrap(Wrap { trim: false }),
        area,
    );
}

fn render_map(frame: &mut ratatui::Frame<'_>, app: &App, area: Rect) {
    let lines = if app.locations.is_empty() {
        vec![Line::from("No locations yet")]
    } else {
        app.locations
            .iter()
            .flat_map(|location| {
                [
                    Line::from(Span::styled(
                        location.name.clone(),
                        Style::default().fg(Color::Yellow),
                    )),
                    Line::from(truncate(&location.description, 120)),
                    Line::from(""),
                ]
            })
            .collect()
    };
    frame.render_widget(
        Paragraph::new(lines)
            .block(Block::default().borders(Borders::ALL).title("Map"))
            .wrap(Wrap { trim: false }),
        area,
    );
}

fn render_events(frame: &mut ratatui::Frame<'_>, app: &App, area: Rect) {
    let lines = if app.events.is_empty() {
        vec![Line::from("No events yet")]
    } else {
        app.events
            .iter()
            .rev()
            .flat_map(|event| {
                [
                    Line::from(Span::styled(
                        format!("importance {}", event.importance),
                        Style::default().fg(Color::DarkGray),
                    )),
                    Line::from(truncate(&event.summary, 120)),
                    Line::from(""),
                ]
            })
            .collect()
    };
    frame.render_widget(
        Paragraph::new(lines)
            .block(Block::default().borders(Borders::ALL).title("Events"))
            .wrap(Wrap { trim: false }),
        area,
    );
}

fn render_context(frame: &mut ratatui::Frame<'_>, app: &App, area: Rect) {
    let mut lines = Vec::new();
    if !app.search_results.is_empty() {
        lines.push(Line::from(Span::styled(
            "Memory Search",
            Style::default().fg(Color::Yellow),
        )));
        for result in app.search_results.iter().take(6) {
            lines.push(Line::from(truncate(result, 110)));
        }
        lines.push(Line::from(""));
    }

    if let Some(preview) = app.context_preview.as_ref() {
        lines.push(Line::from(Span::styled(
            format!("Context Preview: {} tokens", preview.estimated_tokens),
            Style::default().fg(Color::Yellow),
        )));
        for message in preview.messages.iter().take(8) {
            lines.push(Line::from(Span::styled(
                format!("{} [{}]", role_name(message.role), message.estimated_tokens),
                Style::default().fg(Color::DarkGray),
            )));
            lines.push(Line::from(truncate(&message.content, 110)));
        }
    } else if app.search_results.is_empty() {
        lines.push(Line::from("Ctrl-P previews injected context."));
        lines.push(Line::from("Ctrl-F searches memory using composer text."));
    }

    frame.render_widget(
        Paragraph::new(lines)
            .block(Block::default().borders(Borders::ALL).title("Context"))
            .wrap(Wrap { trim: false }),
        area,
    );
}

fn render_composer(frame: &mut ratatui::Frame<'_>, app: &mut App, area: Rect) {
    frame.render_widget(&app.composer, area);
}

fn render_footer(frame: &mut ratatui::Frame<'_>, app: &App, area: Rect) {
    let footer = format!(
        "Enter send | Alt-Enter newline | Ctrl-G game | Ctrl-L session | Ctrl-T panel | Ctrl-P context | Ctrl-F memory | Ctrl-R refresh | ? help | Ctrl-Q quit | {}",
        app.addr
    );
    frame.render_widget(
        Paragraph::new(footer).style(Style::default().fg(Color::DarkGray)),
        area,
    );
}

fn render_finder(frame: &mut ratatui::Frame<'_>, app: &App, area: Rect) {
    let Some(finder) = app.finder.as_ref() else {
        return;
    };
    let popup = centered_rect(70, 70, area);
    frame.render_widget(Clear, popup);
    let rows = match finder.mode {
        FinderMode::Game => finder
            .filtered_games()
            .into_iter()
            .enumerate()
            .map(|(index, game)| finder_item(index, finder.selected, &game.title, &game.id))
            .collect::<Vec<_>>(),
        FinderMode::Session { .. } => finder
            .filtered_sessions()
            .into_iter()
            .enumerate()
            .map(|(index, session)| {
                finder_item(index, finder.selected, &session.title, &session.id)
            })
            .collect::<Vec<_>>(),
    };
    let title = format!("{} - query: {}", finder.title(), finder.query);
    frame.render_widget(
        List::new(rows).block(Block::default().borders(Borders::ALL).title(title)),
        popup,
    );
}

fn finder_item<'a>(index: usize, selected: usize, title: &'a str, id: &'a str) -> ListItem<'a> {
    let style = if index == selected {
        Style::default()
            .fg(Color::Black)
            .bg(Color::Cyan)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
    };
    ListItem::new(Line::from(vec![
        Span::raw(if index == selected { "> " } else { "  " }),
        Span::styled(title.to_owned(), style),
        Span::raw(format!("  {id}")),
    ]))
}

fn render_help(frame: &mut ratatui::Frame<'_>, area: Rect) {
    let popup = centered_rect(62, 54, area);
    frame.render_widget(Clear, popup);
    let lines = vec![
        Line::from("Harpe TUI"),
        Line::from(""),
        Line::from("Enter sends the composer. Alt-Enter or Ctrl-J inserts a newline."),
        Line::from("Ctrl-G opens the game finder; Ctrl-L opens the session finder."),
        Line::from("Ctrl-T cycles Cast/Lore/Map/Events/Context."),
        Line::from("Ctrl-P previews the exact context for the composer text."),
        Line::from("Ctrl-F searches memory using the composer text."),
        Line::from("Ctrl-R refreshes active session data."),
        Line::from("PageUp/PageDown scrolls the transcript."),
        Line::from("Esc closes overlays. Ctrl-Q quits."),
    ];
    frame.render_widget(
        Paragraph::new(lines)
            .block(Block::default().borders(Borders::ALL).title("Help"))
            .wrap(Wrap { trim: false }),
        popup,
    );
}

fn render_error(frame: &mut ratatui::Frame<'_>, error: &str, area: Rect) {
    let popup = centered_rect(60, 25, area);
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(Span::styled("Error", Style::default().fg(Color::Red))),
            Line::from(""),
            Line::from(error.to_owned()),
            Line::from(""),
            Line::from("Press any normal key after closing overlays to continue."),
        ])
        .block(Block::default().borders(Borders::ALL)),
        popup,
    );
}

fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(area);
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(vertical[1])[1]
}

fn role_name(role: i32) -> &'static str {
    match pb::MessageRole::try_from(role).ok() {
        Some(pb::MessageRole::System) => "system",
        Some(pb::MessageRole::User) => "you",
        Some(pb::MessageRole::Assistant) => "narrator",
        Some(pb::MessageRole::Unspecified) | None => "unknown",
    }
}

fn blank_as<'a>(value: &'a str, fallback: &'a str) -> &'a str {
    if value.trim().is_empty() {
        fallback
    } else {
        value
    }
}

fn first_sentences(content: &str, count: usize) -> String {
    let mut sentences = Vec::new();
    let mut start = 0;
    for (index, char) in content.char_indices() {
        if matches!(char, '.' | '!' | '?') {
            let end = index + char.len_utf8();
            let sentence = content[start..end].trim();
            if !sentence.is_empty() {
                sentences.push(sentence.to_owned());
            }
            start = end;
            if sentences.len() >= count {
                break;
            }
        }
    }
    if sentences.is_empty() {
        truncate(content.trim(), 220)
    } else {
        sentences.join(" ")
    }
}

fn truncate(content: &str, limit: usize) -> String {
    let trimmed = content.trim();
    if trimmed.chars().count() <= limit {
        return trimmed.to_owned();
    }
    let mut output = trimmed
        .chars()
        .take(limit.saturating_sub(3))
        .collect::<String>();
    output.push_str("...");
    output
}

fn wrap_owned(content: &str, width: usize) -> Vec<String> {
    let width = width.max(12);
    textwrap::wrap(content, width)
        .into_iter()
        .map(|line| line.into_owned())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
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
