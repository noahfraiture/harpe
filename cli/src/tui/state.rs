use std::cmp::min;

use harpe_proto::pb::{
    self, Character, ContextMessage, Event, Game, HealthCheckResponse, Location, Message, Session,
    StorySummary, WorldFact,
};
use ratatui::style::{Color, Style};
use ratatui::widgets::{Block, Borders};
use tui_textarea::TextArea;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum RightTab {
    Cast,
    Lore,
    Map,
    Events,
    Context,
}

impl RightTab {
    pub(super) const ALL: [Self; 5] = [
        Self::Cast,
        Self::Lore,
        Self::Map,
        Self::Events,
        Self::Context,
    ];

    pub(super) fn title(self) -> &'static str {
        match self {
            Self::Cast => "Cast",
            Self::Lore => "Lore",
            Self::Map => "Map",
            Self::Events => "Events",
            Self::Context => "Context",
        }
    }

    pub(super) fn next(self) -> Self {
        let index = Self::ALL.iter().position(|tab| *tab == self).unwrap_or(0);
        Self::ALL[(index + 1) % Self::ALL.len()]
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum FinderMode {
    Game,
    Session { game_id: String },
}

#[derive(Debug, Clone)]
pub(super) struct FinderState {
    pub(super) mode: FinderMode,
    pub(super) query: String,
    pub(super) selected: usize,
    pub(super) games: Vec<Game>,
    pub(super) sessions: Vec<Session>,
}

impl FinderState {
    pub(super) fn game(games: Vec<Game>) -> Self {
        Self {
            mode: FinderMode::Game,
            query: String::new(),
            selected: 0,
            games,
            sessions: Vec::new(),
        }
    }

    pub(super) fn session(game_id: String, sessions: Vec<Session>) -> Self {
        Self {
            mode: FinderMode::Session { game_id },
            query: String::new(),
            selected: 0,
            games: Vec::new(),
            sessions,
        }
    }

    pub(super) fn title(&self) -> &'static str {
        match self.mode {
            FinderMode::Game => "Find Game",
            FinderMode::Session { .. } => "Find Session",
        }
    }

    pub(super) fn move_down(&mut self) {
        let len = self.filtered_len();
        if len > 0 {
            self.selected = (self.selected + 1) % len;
        }
    }

    pub(super) fn move_up(&mut self) {
        let len = self.filtered_len();
        if len > 0 {
            self.selected = if self.selected == 0 {
                len - 1
            } else {
                self.selected - 1
            };
        }
    }

    pub(super) fn push_query(&mut self, char: char) {
        self.query.push(char);
        self.selected = 0;
    }

    pub(super) fn pop_query(&mut self) {
        self.query.pop();
        self.selected = min(self.selected, self.filtered_len().saturating_sub(1));
    }

    fn filtered_len(&self) -> usize {
        match self.mode {
            FinderMode::Game => self.filtered_games().len(),
            FinderMode::Session { .. } => self.filtered_sessions().len(),
        }
    }

    pub(super) fn filtered_games(&self) -> Vec<&Game> {
        let query = self.query.to_lowercase();
        self.games
            .iter()
            .filter(|game| query.is_empty() || game.title.to_lowercase().contains(&query))
            .collect()
    }

    pub(super) fn filtered_sessions(&self) -> Vec<&Session> {
        let query = self.query.to_lowercase();
        self.sessions
            .iter()
            .filter(|session| query.is_empty() || session.title.to_lowercase().contains(&query))
            .collect()
    }
}

#[derive(Debug, Clone)]
pub(super) struct ContextPreview {
    pub(super) estimated_tokens: u32,
    pub(super) messages: Vec<ContextMessage>,
}

pub(super) struct App {
    pub(super) addr: String,
    pub(super) user_id: String,
    pub(super) model: Option<String>,
    pub(super) game: Option<Game>,
    pub(super) session: Option<Session>,
    pub(super) messages: Vec<Message>,
    pub(super) summary: Option<StorySummary>,
    pub(super) characters: Vec<Character>,
    pub(super) events: Vec<Event>,
    pub(super) facts: Vec<WorldFact>,
    pub(super) locations: Vec<Location>,
    pub(super) context_preview: Option<ContextPreview>,
    pub(super) health: Option<HealthCheckResponse>,
    pub(super) composer: TextArea<'static>,
    pub(super) right_tab: RightTab,
    pub(super) finder: Option<FinderState>,
    pub(super) help_open: bool,
    pub(super) search_results: Vec<String>,
    pub(super) transcript_scroll: u16,
    pub(super) status: String,
    pub(super) error: Option<String>,
    pub(super) streaming: bool,
    pub(super) quit: bool,
}

impl App {
    pub(super) fn new(addr: String, user_id: String, model: Option<String>) -> Self {
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

    pub(super) fn set_error(&mut self, error: impl Into<String>) {
        self.error = Some(error.into());
    }

    pub(super) fn set_status(&mut self, status: impl Into<String>) {
        self.status = status.into();
        self.error = None;
    }

    pub(super) fn composer_content(&self) -> String {
        self.composer.lines().join("\n").trim().to_owned()
    }

    pub(super) fn clear_composer(&mut self) {
        self.composer = TextArea::default();
        self.composer.set_block(
            Block::default()
                .borders(Borders::ALL)
                .title("Composer")
                .border_style(Style::default().fg(Color::DarkGray)),
        );
        self.composer.set_cursor_line_style(Style::default());
    }

    pub(super) fn push_user_message(&mut self, content: String) {
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

    pub(super) fn start_assistant_message(&mut self) {
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

    pub(super) fn append_assistant_delta(&mut self, delta: &str) {
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

pub(super) enum AppEvent {
    AssistantDelta(String),
    SendComplete,
    SendFailed(String),
}
