use futures_util::StreamExt;
use harpe_proto::pb::{
    Character, Event, Game, GetGameRequest, GetSessionRequest, GetStorySummaryRequest,
    HealthCheckRequest, HealthCheckResponse, ListCharactersRequest, ListEventsRequest,
    ListGamesRequest, ListLocationsRequest, ListMessagesRequest, ListSessionsRequest,
    ListWorldFactsRequest, Location, Message, PageRequest, PreviewContextRequest,
    SearchMemoryRequest, SendMessageRequest, Session, StorySummary, WorldFact,
    game_service_client::GameServiceClient, health_service_client::HealthServiceClient,
    memory_service_client::MemoryServiceClient, session_service_client::SessionServiceClient,
};
use tokio::sync::mpsc;
use tonic::transport::Channel;

use super::state::{AppEvent, ContextPreview};
use crate::{CliResult, with_user};

const DEFAULT_PAGE_SIZE: u32 = 50;

#[derive(Clone)]
pub(super) struct TuiClient {
    channel: Channel,
    user_id: String,
}

impl TuiClient {
    pub(super) fn new(channel: Channel, user_id: String) -> Self {
        Self { channel, user_id }
    }

    pub(super) async fn health(&self) -> CliResult<HealthCheckResponse> {
        Ok(HealthServiceClient::new(self.channel.clone())
            .check(HealthCheckRequest {
                service: String::new(),
            })
            .await?
            .into_inner())
    }

    pub(super) async fn list_games(&self) -> CliResult<Vec<Game>> {
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

    pub(super) async fn get_game(&self, game_id: String) -> CliResult<Game> {
        Ok(GameServiceClient::new(self.channel.clone())
            .get_game(with_user(GetGameRequest { game_id }, &self.user_id)?)
            .await?
            .into_inner())
    }

    pub(super) async fn list_sessions(&self, game_id: String) -> CliResult<Vec<Session>> {
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

    pub(super) async fn get_session(&self, session_id: String) -> CliResult<Session> {
        Ok(SessionServiceClient::new(self.channel.clone())
            .get_session(with_user(GetSessionRequest { session_id }, &self.user_id)?)
            .await?
            .into_inner())
    }

    pub(super) async fn list_messages(&self, session_id: String) -> CliResult<Vec<Message>> {
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

    pub(super) async fn summary(&self, session_id: String) -> CliResult<StorySummary> {
        Ok(MemoryServiceClient::new(self.channel.clone())
            .get_story_summary(with_user(
                GetStorySummaryRequest { session_id },
                &self.user_id,
            )?)
            .await?
            .into_inner())
    }

    pub(super) async fn characters(&self, game_id: String) -> CliResult<Vec<Character>> {
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

    pub(super) async fn events(&self, session_id: String) -> CliResult<Vec<Event>> {
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

    pub(super) async fn facts(&self, game_id: String) -> CliResult<Vec<WorldFact>> {
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

    pub(super) async fn locations(&self, game_id: String) -> CliResult<Vec<Location>> {
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

    pub(super) async fn preview_context(
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

    pub(super) async fn search_memory(
        &self,
        session_id: String,
        query: String,
    ) -> CliResult<Vec<String>> {
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

    pub(super) async fn stream_message(
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
