use std::sync::Arc;

use chrono::Utc;
use futures_util::StreamExt;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tonic::{Request, Response, Status, metadata::MetadataMap};

use crate::domain::{
    BackgroundJob, Character, Event, Game, GameSnapshot, JobKind, JobStatus, Location, MemoryChunk,
    MemoryHit, Message, MessageRole, NewGame, NewMessage, NewSession, NewUser, Session,
    StorySummary, User, WorldFact, new_id,
};
use crate::engine::{ContextBuilder, ContextInputs, estimate_tokens};
use crate::jobs::{UpdateMemoryAfterTurnPayload, new_update_memory_job};
use crate::llm::{ChatRequest, LlmClient};
use crate::observability::{AppMetrics, MetricsSnapshot as AppMetricsSnapshot, SharedMetrics};
use crate::pb::{
    self, ContextMessage, CreateGameRequest, CreateSessionRequest, CreateUserRequest,
    ExportGameRequest, GetCharacterRequest, GetGameRequest, GetMetricsRequest, GetSessionRequest,
    GetStorySummaryRequest, GetUserRequest, HealthCheckRequest, ListBackgroundJobsRequest,
    ListCharactersRequest, ListEventsRequest, ListGamesRequest, ListLocationsRequest,
    ListMemoryChunksRequest, ListMessagesRequest, ListSessionsRequest, ListWorldFactsRequest,
    MessageDelta, PreviewContextRequest, SearchMemoryRequest, SendMessageRequest,
    admin_service_server, game_service_server, health_service_server, memory_service_server,
    metrics_service_server, session_service_server, user_service_server,
};
use crate::store::HarpeStore;
use crate::{HarpeError, Result};

#[derive(Clone)]
pub struct HarpeGrpc {
    store: Arc<dyn HarpeStore>,
    llm: Arc<dyn LlmClient>,
    context_builder: ContextBuilder,
    metrics: SharedMetrics,
}

impl HarpeGrpc {
    pub fn new(store: Arc<dyn HarpeStore>, llm: Arc<dyn LlmClient>) -> Self {
        Self {
            store,
            llm,
            context_builder: ContextBuilder::default(),
            metrics: AppMetrics::shared(),
        }
    }

    pub fn with_context_builder(mut self, context_builder: ContextBuilder) -> Self {
        self.context_builder = context_builder;
        self
    }

    pub fn with_metrics(mut self, metrics: SharedMetrics) -> Self {
        self.metrics = metrics;
        self
    }
}

#[tonic::async_trait]
impl user_service_server::UserService for HarpeGrpc {
    async fn create_user(
        &self,
        request: Request<CreateUserRequest>,
    ) -> std::result::Result<Response<pb::User>, Status> {
        self.metrics.record_grpc_request();
        tracing::debug!(rpc = "UserService.CreateUser");
        let request = request.into_inner();
        let user = self
            .store
            .create_user(NewUser {
                display_name: request.display_name,
            })
            .await
            .map_err(status_from_error)?;

        Ok(Response::new(user_to_pb(user)))
    }

    async fn get_user(
        &self,
        request: Request<GetUserRequest>,
    ) -> std::result::Result<Response<pb::User>, Status> {
        self.metrics.record_grpc_request();
        tracing::debug!(rpc = "UserService.GetUser");
        let user = self
            .store
            .get_user(&request.into_inner().user_id)
            .await
            .map_err(status_from_error)?;

        Ok(Response::new(user_to_pb(user)))
    }
}

#[tonic::async_trait]
impl game_service_server::GameService for HarpeGrpc {
    async fn create_game(
        &self,
        request: Request<CreateGameRequest>,
    ) -> std::result::Result<Response<pb::Game>, Status> {
        self.metrics.record_grpc_request();
        tracing::debug!(rpc = "GameService.CreateGame");
        let metadata_user_id = optional_user_id(request.metadata()).map_err(status_from_error)?;
        let request = request.into_inner();
        let owner_user_id =
            resolve_owner_user_id(metadata_user_id.as_deref(), &request.owner_user_id)
                .map_err(status_from_error)?;
        let game = self
            .store
            .create_game(NewGame {
                owner_user_id,
                title: request.title,
                system_prompt: request.system_prompt,
            })
            .await
            .map_err(status_from_error)?;

        Ok(Response::new(game_to_pb(game)))
    }

    async fn list_games(
        &self,
        request: Request<ListGamesRequest>,
    ) -> std::result::Result<Response<pb::ListGamesResponse>, Status> {
        self.metrics.record_grpc_request();
        tracing::debug!(rpc = "GameService.ListGames");
        let user_id = require_user_id(request.metadata()).map_err(status_from_error)?;
        let request = request.into_inner();
        let games = self
            .store
            .list_games_for_user(
                &user_id,
                request_limit(request.limit, request.page.as_ref()),
            )
            .await
            .map_err(status_from_error)?
            .into_iter()
            .map(game_to_pb)
            .collect::<Vec<_>>();
        let page = Some(page_info(games.len()));

        Ok(Response::new(pb::ListGamesResponse { games, page }))
    }

    async fn get_game(
        &self,
        request: Request<GetGameRequest>,
    ) -> std::result::Result<Response<pb::Game>, Status> {
        self.metrics.record_grpc_request();
        tracing::debug!(rpc = "GameService.GetGame");
        let user_id = require_user_id(request.metadata()).map_err(status_from_error)?;
        let game_id = request.into_inner().game_id;
        let game = require_owned_game(self.store.as_ref(), &game_id, &user_id)
            .await
            .map_err(status_from_error)?;

        Ok(Response::new(game_to_pb(game)))
    }
}

#[tonic::async_trait]
impl session_service_server::SessionService for HarpeGrpc {
    type SendMessageStream = ReceiverStream<std::result::Result<MessageDelta, Status>>;

    async fn create_session(
        &self,
        request: Request<CreateSessionRequest>,
    ) -> std::result::Result<Response<pb::Session>, Status> {
        self.metrics.record_grpc_request();
        tracing::debug!(rpc = "SessionService.CreateSession");
        let user_id = require_user_id(request.metadata()).map_err(status_from_error)?;
        let request = request.into_inner();
        require_owned_game(self.store.as_ref(), &request.game_id, &user_id)
            .await
            .map_err(status_from_error)?;
        let session = self
            .store
            .create_session(NewSession {
                game_id: request.game_id,
                title: request.title,
            })
            .await
            .map_err(status_from_error)?;

        Ok(Response::new(session_to_pb(session)))
    }

    async fn list_sessions(
        &self,
        request: Request<ListSessionsRequest>,
    ) -> std::result::Result<Response<pb::ListSessionsResponse>, Status> {
        self.metrics.record_grpc_request();
        tracing::debug!(rpc = "SessionService.ListSessions");
        let user_id = require_user_id(request.metadata()).map_err(status_from_error)?;
        let request = request.into_inner();
        require_owned_game(self.store.as_ref(), &request.game_id, &user_id)
            .await
            .map_err(status_from_error)?;
        let sessions = self
            .store
            .list_sessions(
                &request.game_id,
                request_limit(request.limit, request.page.as_ref()),
            )
            .await
            .map_err(status_from_error)?
            .into_iter()
            .map(session_to_pb)
            .collect::<Vec<_>>();
        let page = Some(page_info(sessions.len()));

        Ok(Response::new(pb::ListSessionsResponse { sessions, page }))
    }

    async fn get_session(
        &self,
        request: Request<GetSessionRequest>,
    ) -> std::result::Result<Response<pb::Session>, Status> {
        self.metrics.record_grpc_request();
        tracing::debug!(rpc = "SessionService.GetSession");
        let user_id = require_user_id(request.metadata()).map_err(status_from_error)?;
        let session_id = request.into_inner().session_id;
        let session = require_owned_session(self.store.as_ref(), &session_id, &user_id)
            .await
            .map_err(status_from_error)?;

        Ok(Response::new(session_to_pb(session)))
    }

    async fn send_message(
        &self,
        request: Request<SendMessageRequest>,
    ) -> std::result::Result<Response<Self::SendMessageStream>, Status> {
        self.metrics.record_grpc_request();
        tracing::debug!(rpc = "SessionService.SendMessage");
        let user_id = require_user_id(request.metadata()).map_err(status_from_error)?;
        let request = request.into_inner();
        let store = Arc::clone(&self.store);
        let llm = Arc::clone(&self.llm);
        let metrics = Arc::clone(&self.metrics);
        let context_builder = self.context_builder.clone();
        let (tx, rx) = mpsc::channel(16);

        tokio::spawn(async move {
            if let Err(error) = run_send_message(
                request,
                user_id,
                store,
                llm,
                metrics.clone(),
                context_builder,
                tx.clone(),
            )
            .await
            {
                metrics.record_grpc_failure();
                let _ = tx.send(Err(status_from_error(error))).await;
            }
        });

        Ok(Response::new(ReceiverStream::new(rx)))
    }

    async fn preview_context(
        &self,
        request: Request<PreviewContextRequest>,
    ) -> std::result::Result<Response<pb::PreviewContextResponse>, Status> {
        self.metrics.record_grpc_request();
        tracing::debug!(rpc = "SessionService.PreviewContext");
        let user_id = require_user_id(request.metadata()).map_err(status_from_error)?;
        let request = request.into_inner();
        if request.content.trim().is_empty() {
            return Err(status_from_error(HarpeError::Validation(
                "message content is required".to_owned(),
            )));
        }

        let session = require_owned_session(self.store.as_ref(), &request.session_id, &user_id)
            .await
            .map_err(status_from_error)?;
        let game = self
            .store
            .get_game(&session.game_id)
            .await
            .map_err(status_from_error)?;
        let chat_request = build_context_for_turn(
            &session,
            &game,
            &request.content,
            true,
            self.store.as_ref(),
            self.llm.as_ref(),
            &self.context_builder,
        )
        .await
        .map_err(status_from_error)?;
        let response = preview_context_to_pb(chat_request);

        Ok(Response::new(response))
    }

    async fn list_messages(
        &self,
        request: Request<ListMessagesRequest>,
    ) -> std::result::Result<Response<pb::ListMessagesResponse>, Status> {
        self.metrics.record_grpc_request();
        tracing::debug!(rpc = "SessionService.ListMessages");
        let user_id = require_user_id(request.metadata()).map_err(status_from_error)?;
        let request = request.into_inner();
        require_owned_session(self.store.as_ref(), &request.session_id, &user_id)
            .await
            .map_err(status_from_error)?;
        let messages = self
            .store
            .list_recent_messages(
                &request.session_id,
                request_limit(request.limit, request.page.as_ref()),
            )
            .await
            .map_err(status_from_error)?
            .into_iter()
            .map(message_to_pb)
            .collect::<Vec<_>>();
        let page = Some(page_info(messages.len()));

        Ok(Response::new(pb::ListMessagesResponse { messages, page }))
    }
}

#[tonic::async_trait]
impl memory_service_server::MemoryService for HarpeGrpc {
    async fn get_story_summary(
        &self,
        request: Request<GetStorySummaryRequest>,
    ) -> std::result::Result<Response<pb::StorySummary>, Status> {
        self.metrics.record_grpc_request();
        tracing::debug!(rpc = "MemoryService.GetStorySummary");
        let user_id = require_user_id(request.metadata()).map_err(status_from_error)?;
        let request = request.into_inner();
        require_owned_session(self.store.as_ref(), &request.session_id, &user_id)
            .await
            .map_err(status_from_error)?;
        let summary = self
            .store
            .get_story_summary(&request.session_id)
            .await
            .map_err(status_from_error)?
            .ok_or_else(|| Status::not_found(format!("summary {}", request.session_id)))?;

        Ok(Response::new(summary_to_pb(summary)))
    }

    async fn list_characters(
        &self,
        request: Request<ListCharactersRequest>,
    ) -> std::result::Result<Response<pb::ListCharactersResponse>, Status> {
        self.metrics.record_grpc_request();
        tracing::debug!(rpc = "MemoryService.ListCharacters");
        let user_id = require_user_id(request.metadata()).map_err(status_from_error)?;
        let request = request.into_inner();
        require_owned_game(self.store.as_ref(), &request.game_id, &user_id)
            .await
            .map_err(status_from_error)?;
        let mut characters = self
            .store
            .list_characters(&request.game_id)
            .await
            .map_err(status_from_error)?;
        truncate_to_limit(
            &mut characters,
            request_limit(request.limit, request.page.as_ref()),
        );
        let characters = characters
            .into_iter()
            .map(character_to_pb)
            .collect::<Vec<_>>();
        let page = Some(page_info(characters.len()));

        Ok(Response::new(pb::ListCharactersResponse {
            characters,
            page,
        }))
    }

    async fn get_character(
        &self,
        request: Request<GetCharacterRequest>,
    ) -> std::result::Result<Response<pb::Character>, Status> {
        self.metrics.record_grpc_request();
        tracing::debug!(rpc = "MemoryService.GetCharacter");
        let user_id = require_user_id(request.metadata()).map_err(status_from_error)?;
        let character = self
            .store
            .get_character(&request.into_inner().character_id)
            .await
            .map_err(status_from_error)?;
        require_owned_game(self.store.as_ref(), &character.game_id, &user_id)
            .await
            .map_err(status_from_error)?;

        Ok(Response::new(character_to_pb(character)))
    }

    async fn list_events(
        &self,
        request: Request<ListEventsRequest>,
    ) -> std::result::Result<Response<pb::ListEventsResponse>, Status> {
        self.metrics.record_grpc_request();
        tracing::debug!(rpc = "MemoryService.ListEvents");
        let user_id = require_user_id(request.metadata()).map_err(status_from_error)?;
        let request = request.into_inner();
        require_owned_session(self.store.as_ref(), &request.session_id, &user_id)
            .await
            .map_err(status_from_error)?;
        let events = self
            .store
            .list_events(
                &request.session_id,
                request_limit(request.limit, request.page.as_ref()),
            )
            .await
            .map_err(status_from_error)?
            .into_iter()
            .map(event_to_pb)
            .collect::<Vec<_>>();
        let page = Some(page_info(events.len()));

        Ok(Response::new(pb::ListEventsResponse { events, page }))
    }

    async fn list_world_facts(
        &self,
        request: Request<ListWorldFactsRequest>,
    ) -> std::result::Result<Response<pb::ListWorldFactsResponse>, Status> {
        self.metrics.record_grpc_request();
        tracing::debug!(rpc = "MemoryService.ListWorldFacts");
        let user_id = require_user_id(request.metadata()).map_err(status_from_error)?;
        let request = request.into_inner();
        require_owned_game(self.store.as_ref(), &request.game_id, &user_id)
            .await
            .map_err(status_from_error)?;
        let facts = self
            .store
            .list_world_facts(
                &request.game_id,
                request_limit(request.limit, request.page.as_ref()),
            )
            .await
            .map_err(status_from_error)?
            .into_iter()
            .map(world_fact_to_pb)
            .collect::<Vec<_>>();
        let page = Some(page_info(facts.len()));

        Ok(Response::new(pb::ListWorldFactsResponse { facts, page }))
    }

    async fn list_locations(
        &self,
        request: Request<ListLocationsRequest>,
    ) -> std::result::Result<Response<pb::ListLocationsResponse>, Status> {
        self.metrics.record_grpc_request();
        tracing::debug!(rpc = "MemoryService.ListLocations");
        let user_id = require_user_id(request.metadata()).map_err(status_from_error)?;
        let request = request.into_inner();
        require_owned_game(self.store.as_ref(), &request.game_id, &user_id)
            .await
            .map_err(status_from_error)?;
        let mut locations = self
            .store
            .list_locations(&request.game_id)
            .await
            .map_err(status_from_error)?;
        truncate_to_limit(
            &mut locations,
            request_limit(request.limit, request.page.as_ref()),
        );
        let locations = locations
            .into_iter()
            .map(location_to_pb)
            .collect::<Vec<_>>();
        let page = Some(page_info(locations.len()));

        Ok(Response::new(pb::ListLocationsResponse { locations, page }))
    }

    async fn search_memory(
        &self,
        request: Request<SearchMemoryRequest>,
    ) -> std::result::Result<Response<pb::SearchMemoryResponse>, Status> {
        self.metrics.record_grpc_request();
        tracing::debug!(rpc = "MemoryService.SearchMemory");
        let user_id = require_user_id(request.metadata()).map_err(status_from_error)?;
        let request = request.into_inner();
        require_owned_session(self.store.as_ref(), &request.session_id, &user_id)
            .await
            .map_err(status_from_error)?;
        let embedding = self
            .llm
            .embed(&request.query)
            .await
            .map_err(status_from_error)?;
        let hits = self
            .store
            .search_memory(
                &request.session_id,
                &request.query,
                &embedding,
                request_limit(request.limit, request.page.as_ref()),
            )
            .await
            .map_err(status_from_error)?
            .into_iter()
            .map(memory_hit_to_pb)
            .collect::<Vec<_>>();
        let page = Some(page_info(hits.len()));

        Ok(Response::new(pb::SearchMemoryResponse { hits, page }))
    }

    async fn export_game(
        &self,
        request: Request<ExportGameRequest>,
    ) -> std::result::Result<Response<pb::GameSnapshot>, Status> {
        self.metrics.record_grpc_request();
        tracing::debug!(rpc = "MemoryService.ExportGame");
        let user_id = require_user_id(request.metadata()).map_err(status_from_error)?;
        let request = request.into_inner();
        require_owned_game(self.store.as_ref(), &request.game_id, &user_id)
            .await
            .map_err(status_from_error)?;
        let snapshot = self
            .store
            .export_game_snapshot(&request.game_id)
            .await
            .map_err(status_from_error)?;

        Ok(Response::new(snapshot_to_pb(snapshot)))
    }
}

#[tonic::async_trait]
impl health_service_server::HealthService for HarpeGrpc {
    async fn check(
        &self,
        request: Request<HealthCheckRequest>,
    ) -> std::result::Result<Response<pb::HealthCheckResponse>, Status> {
        self.metrics.record_grpc_request();
        self.metrics.record_health_check();
        tracing::debug!(rpc = "HealthService.Check");
        let service = normalize_health_service(&request.into_inner().service);
        let response = health_response(self.store.as_ref(), service).await;

        Ok(Response::new(response))
    }
}

#[tonic::async_trait]
impl metrics_service_server::MetricsService for HarpeGrpc {
    async fn get_metrics(
        &self,
        _request: Request<GetMetricsRequest>,
    ) -> std::result::Result<Response<pb::MetricsSnapshot>, Status> {
        self.metrics.record_grpc_request();
        tracing::debug!(rpc = "MetricsService.GetMetrics");

        Ok(Response::new(metrics_to_pb(self.metrics.snapshot())))
    }
}

#[tonic::async_trait]
impl admin_service_server::AdminService for HarpeGrpc {
    async fn list_background_jobs(
        &self,
        request: Request<ListBackgroundJobsRequest>,
    ) -> std::result::Result<Response<pb::ListBackgroundJobsResponse>, Status> {
        self.metrics.record_grpc_request();
        tracing::debug!(rpc = "AdminService.ListBackgroundJobs");
        let request = request.into_inner();
        let status = admin_job_status_filter(request.status).map_err(status_from_error)?;
        let jobs = self
            .store
            .list_jobs(status, request_limit(request.limit, request.page.as_ref()))
            .await
            .map_err(status_from_error)?
            .into_iter()
            .map(background_job_to_pb)
            .collect::<Vec<_>>();
        let page = Some(page_info(jobs.len()));

        Ok(Response::new(pb::ListBackgroundJobsResponse { jobs, page }))
    }

    async fn list_memory_chunks(
        &self,
        request: Request<ListMemoryChunksRequest>,
    ) -> std::result::Result<Response<pb::ListMemoryChunksResponse>, Status> {
        self.metrics.record_grpc_request();
        tracing::debug!(rpc = "AdminService.ListMemoryChunks");
        let request = request.into_inner();
        if request.session_id.trim().is_empty() {
            return Err(status_from_error(HarpeError::Validation(
                "session id is required".to_owned(),
            )));
        }

        let chunks = self
            .store
            .list_memory_chunks(
                &request.session_id,
                request_limit(request.limit, request.page.as_ref()),
            )
            .await
            .map_err(status_from_error)?
            .into_iter()
            .map(memory_chunk_to_pb)
            .collect::<Vec<_>>();
        let page = Some(page_info(chunks.len()));

        Ok(Response::new(pb::ListMemoryChunksResponse { chunks, page }))
    }
}

async fn run_send_message(
    request: SendMessageRequest,
    user_id: String,
    store: Arc<dyn HarpeStore>,
    llm: Arc<dyn LlmClient>,
    metrics: SharedMetrics,
    context_builder: ContextBuilder,
    tx: mpsc::Sender<std::result::Result<MessageDelta, Status>>,
) -> Result<()> {
    if request.content.trim().is_empty() {
        return Err(HarpeError::Validation(
            "message content is required".to_owned(),
        ));
    }

    let session = require_owned_session(store.as_ref(), &request.session_id, &user_id).await?;
    let game = store.get_game(&session.game_id).await?;

    store
        .append_message(NewMessage {
            id: None,
            session_id: session.id.clone(),
            role: MessageRole::User,
            content: request.content.clone(),
        })
        .await?;

    let chat_request = build_context_for_turn(
        &session,
        &game,
        &request.content,
        false,
        store.as_ref(),
        llm.as_ref(),
        &context_builder,
    )
    .await?;

    let assistant_id = new_id();
    let mut response_stream = llm.stream_chat(chat_request).await?;
    let mut assistant_content = String::new();
    let mut sequence = 0_u32;

    while let Some(delta) = response_stream.next().await {
        let delta = delta?;
        assistant_content.push_str(&delta);
        sequence = sequence.saturating_add(1);

        if tx
            .send(Ok(MessageDelta {
                session_id: session.id.clone(),
                message_id: assistant_id.clone(),
                delta,
                done: false,
                sequence,
                finish_reason: pb::MessageFinishReason::InProgress as i32,
            }))
            .await
            .is_err()
        {
            return Ok(());
        }
        metrics.record_streamed_message();
    }

    if assistant_content.trim().is_empty() {
        return Err(HarpeError::Llm("assistant response was empty".to_owned()));
    }

    store
        .append_message(NewMessage {
            id: Some(assistant_id.clone()),
            session_id: session.id.clone(),
            role: MessageRole::Assistant,
            content: assistant_content.clone(),
        })
        .await?;

    store
        .enqueue_job(new_update_memory_job(UpdateMemoryAfterTurnPayload::new(
            game.id,
            session.id.clone(),
            assistant_id.clone(),
            assistant_content,
        ))?)
        .await?;

    let _ = tx
        .send(Ok(MessageDelta {
            session_id: session.id,
            message_id: assistant_id,
            delta: String::new(),
            done: true,
            sequence: sequence.saturating_add(1),
            finish_reason: pb::MessageFinishReason::AssistantComplete as i32,
        }))
        .await;

    Ok(())
}

async fn build_context_for_turn(
    session: &Session,
    game: &Game,
    user_content: &str,
    include_ephemeral_user_message: bool,
    store: &dyn HarpeStore,
    llm: &dyn LlmClient,
    context_builder: &ContextBuilder,
) -> Result<ChatRequest> {
    let query_embedding = llm.embed(user_content).await?;
    let summary = store.get_story_summary(&session.id).await?;
    let recent_events = store.list_events(&session.id, 12).await?;
    let memories = store
        .search_memory(
            &session.id,
            user_content,
            &query_embedding,
            context_builder.memory_limit,
        )
        .await?;
    let characters = store.list_characters(&game.id).await?;
    let world_facts = store.list_world_facts(&game.id, 24).await?;
    let locations = store.list_locations(&game.id).await?;
    let mut recent_messages = store
        .list_recent_messages(&session.id, context_builder.recent_message_limit)
        .await?;

    if include_ephemeral_user_message {
        recent_messages.push(Message {
            id: String::new(),
            session_id: session.id.clone(),
            role: MessageRole::User,
            content: user_content.to_owned(),
            created_at: Utc::now(),
        });
    }

    Ok(context_builder.build(ContextInputs {
        base_system_prompt: game.system_prompt.clone(),
        summary,
        recent_events,
        memories,
        characters,
        world_facts,
        locations,
        recent_messages,
    }))
}

fn optional_user_id(metadata: &MetadataMap) -> Result<Option<String>> {
    let Some(value) = metadata.get("x-user-id") else {
        return Ok(None);
    };

    let value = value
        .to_str()
        .map_err(|_| HarpeError::PermissionDenied("x-user-id metadata is invalid".to_owned()))?
        .trim()
        .to_owned();

    Ok((!value.is_empty()).then_some(value))
}

fn require_user_id(metadata: &MetadataMap) -> Result<String> {
    optional_user_id(metadata)?
        .ok_or_else(|| HarpeError::PermissionDenied("x-user-id metadata is required".to_owned()))
}

fn resolve_owner_user_id(
    metadata_user_id: Option<&str>,
    request_owner_user_id: &str,
) -> Result<String> {
    let request_owner_user_id = request_owner_user_id.trim();

    match (metadata_user_id, request_owner_user_id.is_empty()) {
        (Some(user_id), true) => Ok(user_id.to_owned()),
        (Some(user_id), false) if user_id == request_owner_user_id => Ok(user_id.to_owned()),
        (Some(_), false) => Err(HarpeError::PermissionDenied(
            "owner_user_id must match x-user-id metadata".to_owned(),
        )),
        (None, false) => Ok(request_owner_user_id.to_owned()),
        (None, true) => Err(HarpeError::PermissionDenied(
            "owner_user_id or x-user-id metadata is required".to_owned(),
        )),
    }
}

async fn require_owned_session(
    store: &dyn HarpeStore,
    session_id: &str,
    user_id: &str,
) -> Result<Session> {
    let session = store.get_session(session_id).await?;
    require_owned_game(store, &session.game_id, user_id).await?;

    Ok(session)
}

async fn require_owned_game(store: &dyn HarpeStore, game_id: &str, user_id: &str) -> Result<Game> {
    let game = store.get_game(game_id).await?;
    if game.owner_user_id == user_id {
        return Ok(game);
    }

    Err(HarpeError::PermissionDenied(format!("game {game_id}")))
}

async fn health_response(store: &dyn HarpeStore, service: String) -> pb::HealthCheckResponse {
    let checked_at = Utc::now().to_rfc3339();
    let health = async {
        store.list_games(1).await?;
        let pending_jobs = store
            .list_jobs(Some(JobStatus::Pending), 1_000)
            .await?
            .len();
        let failed_jobs = store.list_jobs(Some(JobStatus::Failed), 1_000).await?.len();
        Result::Ok((pending_jobs, failed_jobs))
    }
    .await;

    match health {
        Ok((pending_jobs, failed_jobs)) => {
            let status = if failed_jobs > 0 {
                pb::ServingStatus::Degraded
            } else {
                pb::ServingStatus::Serving
            };

            pb::HealthCheckResponse {
                status: status as i32,
                service,
                version: env!("CARGO_PKG_VERSION").to_owned(),
                database_ok: true,
                pending_jobs: saturating_u32(pending_jobs),
                failed_jobs: saturating_u32(failed_jobs),
                checked_at,
            }
        }
        Err(error) => pb::HealthCheckResponse {
            status: pb::ServingStatus::NotServing as i32,
            service,
            version: env!("CARGO_PKG_VERSION").to_owned(),
            database_ok: false,
            pending_jobs: 0,
            failed_jobs: 0,
            checked_at: format!("{checked_at}; error={error}"),
        },
    }
}

fn normalize_health_service(service: &str) -> String {
    let service = service.trim();
    if service.is_empty() {
        "harpe.v1".to_owned()
    } else {
        service.to_owned()
    }
}

fn limit_from_u32(limit: u32) -> usize {
    usize::try_from(limit).unwrap_or(usize::MAX)
}

fn request_limit(legacy_limit: u32, page: Option<&pb::PageRequest>) -> usize {
    let page_size = page
        .and_then(|page| (page.page_size > 0).then_some(page.page_size))
        .unwrap_or(legacy_limit);

    limit_from_u32(page_size)
}

fn truncate_to_limit<T>(items: &mut Vec<T>, limit: usize) {
    if limit > 0 {
        items.truncate(limit);
    }
}

fn page_info(returned_count: usize) -> pb::PageInfo {
    pb::PageInfo {
        next_page_token: String::new(),
        returned_count: saturating_u32(returned_count),
    }
}

fn status_from_error(error: HarpeError) -> Status {
    match error {
        HarpeError::Validation(message) => Status::invalid_argument(message),
        HarpeError::NotFound(message) => Status::not_found(message),
        HarpeError::PermissionDenied(message) => Status::permission_denied(message),
        HarpeError::Store(message) => Status::internal(message),
        HarpeError::Llm(message) => Status::unavailable(message),
    }
}

fn user_to_pb(user: User) -> pb::User {
    pb::User {
        id: user.id,
        display_name: user.display_name,
        created_at: user.created_at.to_rfc3339(),
    }
}

fn game_to_pb(game: Game) -> pb::Game {
    pb::Game {
        id: game.id,
        title: game.title,
        system_prompt: game.system_prompt,
        created_at: game.created_at.to_rfc3339(),
        owner_user_id: game.owner_user_id,
    }
}

fn session_to_pb(session: Session) -> pb::Session {
    pb::Session {
        id: session.id,
        game_id: session.game_id,
        title: session.title,
        created_at: session.created_at.to_rfc3339(),
    }
}

fn message_to_pb(message: Message) -> pb::Message {
    pb::Message {
        id: message.id,
        session_id: message.session_id,
        role: role_to_pb(message.role),
        content: message.content,
        created_at: message.created_at.to_rfc3339(),
    }
}

fn summary_to_pb(summary: StorySummary) -> pb::StorySummary {
    pb::StorySummary {
        session_id: summary.session_id,
        content: summary.content,
        updated_at: summary.updated_at.to_rfc3339(),
    }
}

fn character_to_pb(character: Character) -> pb::Character {
    pb::Character {
        id: character.id,
        game_id: character.game_id,
        name: character.name,
        description: character.description,
        status: character.status,
        updated_at: character.updated_at.to_rfc3339(),
    }
}

fn event_to_pb(event: Event) -> pb::Event {
    pb::Event {
        id: event.id,
        session_id: event.session_id,
        summary: event.summary,
        importance: event.importance,
        created_at: event.created_at.to_rfc3339(),
    }
}

fn world_fact_to_pb(fact: WorldFact) -> pb::WorldFact {
    pb::WorldFact {
        id: fact.id,
        game_id: fact.game_id,
        subject: fact.subject,
        predicate: fact.predicate,
        object: fact.object,
        content: fact.content,
        confidence: fact.confidence,
        updated_at: fact.updated_at.to_rfc3339(),
    }
}

fn location_to_pb(location: Location) -> pb::Location {
    pb::Location {
        id: location.id,
        game_id: location.game_id,
        name: location.name,
        description: location.description,
        updated_at: location.updated_at.to_rfc3339(),
    }
}

fn memory_hit_to_pb(hit: MemoryHit) -> pb::MemoryHit {
    pb::MemoryHit {
        id: hit.chunk.id,
        session_id: hit.chunk.session_id,
        kind: hit.chunk.kind,
        content: hit.chunk.content,
        score: hit.score,
    }
}

fn memory_chunk_to_pb(chunk: MemoryChunk) -> pb::MemoryChunk {
    pb::MemoryChunk {
        id: chunk.id,
        session_id: chunk.session_id,
        kind: chunk.kind,
        content: chunk.content,
        embedding: chunk.embedding,
        created_at: chunk.created_at.to_rfc3339(),
    }
}

fn background_job_to_pb(job: BackgroundJob) -> pb::BackgroundJobDebug {
    pb::BackgroundJobDebug {
        id: job.id,
        kind: admin_job_kind_to_pb(job.kind),
        status: admin_job_status_to_pb(job.status),
        payload_json: serde_json::to_string(&job.payload)
            .unwrap_or_else(|error| format!("{{\"error\":\"{error}\"}}")),
        attempts: job.attempts,
        max_attempts: job.max_attempts,
        last_error: job.last_error.unwrap_or_default(),
        run_after: job.run_after.to_rfc3339(),
        created_at: job.created_at.to_rfc3339(),
        updated_at: job.updated_at.to_rfc3339(),
    }
}

fn snapshot_to_pb(snapshot: GameSnapshot) -> pb::GameSnapshot {
    pb::GameSnapshot {
        game: Some(game_to_pb(snapshot.game)),
        sessions: snapshot.sessions.into_iter().map(session_to_pb).collect(),
        summaries: snapshot.summaries.into_iter().map(summary_to_pb).collect(),
        characters: snapshot
            .characters
            .into_iter()
            .map(character_to_pb)
            .collect(),
        events: snapshot.events.into_iter().map(event_to_pb).collect(),
        world_facts: snapshot
            .world_facts
            .into_iter()
            .map(world_fact_to_pb)
            .collect(),
        locations: snapshot.locations.into_iter().map(location_to_pb).collect(),
        memory_chunks: snapshot
            .memory_chunks
            .into_iter()
            .map(memory_chunk_to_pb)
            .collect(),
        exported_at: snapshot.exported_at.to_rfc3339(),
    }
}

fn metrics_to_pb(snapshot: AppMetricsSnapshot) -> pb::MetricsSnapshot {
    pb::MetricsSnapshot {
        grpc_requests: snapshot.grpc_requests,
        grpc_failures: snapshot.grpc_failures,
        streamed_messages: snapshot.streamed_messages,
        jobs_processed: snapshot.jobs_processed,
        jobs_succeeded: snapshot.jobs_succeeded,
        jobs_retried: snapshot.jobs_retried,
        jobs_failed: snapshot.jobs_failed,
        health_checks: snapshot.health_checks,
        collected_at: snapshot.collected_at.to_rfc3339(),
    }
}

fn preview_context_to_pb(chat_request: ChatRequest) -> pb::PreviewContextResponse {
    let mut estimated_total = 0_usize;
    let messages = chat_request
        .messages
        .into_iter()
        .map(|message| {
            let estimated = estimate_tokens(&message.content);
            estimated_total = estimated_total.saturating_add(estimated);

            ContextMessage {
                role: role_to_pb(message.role),
                content: message.content,
                estimated_tokens: saturating_u32(estimated),
            }
        })
        .collect();

    pb::PreviewContextResponse {
        messages,
        estimated_tokens: saturating_u32(estimated_total),
    }
}

fn role_to_pb(role: MessageRole) -> i32 {
    match role {
        MessageRole::System => pb::MessageRole::System as i32,
        MessageRole::User => pb::MessageRole::User as i32,
        MessageRole::Assistant => pb::MessageRole::Assistant as i32,
    }
}

fn admin_job_kind_to_pb(kind: JobKind) -> i32 {
    match kind {
        JobKind::UpdateMemoryAfterTurn => pb::AdminJobKind::UpdateMemoryAfterTurn as i32,
    }
}

fn admin_job_status_to_pb(status: JobStatus) -> i32 {
    match status {
        JobStatus::Pending => pb::AdminJobStatus::Pending as i32,
        JobStatus::Running => pb::AdminJobStatus::Running as i32,
        JobStatus::Succeeded => pb::AdminJobStatus::Succeeded as i32,
        JobStatus::Failed => pb::AdminJobStatus::Failed as i32,
    }
}

fn admin_job_status_filter(status: i32) -> Result<Option<JobStatus>> {
    match pb::AdminJobStatus::try_from(status) {
        Ok(pb::AdminJobStatus::Unspecified) => Ok(None),
        Ok(pb::AdminJobStatus::Pending) => Ok(Some(JobStatus::Pending)),
        Ok(pb::AdminJobStatus::Running) => Ok(Some(JobStatus::Running)),
        Ok(pb::AdminJobStatus::Succeeded) => Ok(Some(JobStatus::Succeeded)),
        Ok(pb::AdminJobStatus::Failed) => Ok(Some(JobStatus::Failed)),
        Err(_) => Err(HarpeError::Validation(format!(
            "unknown admin job status {status}"
        ))),
    }
}

fn saturating_u32(value: usize) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}

#[cfg(test)]
mod tests {
    use crate::llm::ChatMessage;

    use super::*;

    #[test]
    fn owner_user_id_uses_metadata_and_rejects_mismatch() {
        assert_eq!(resolve_owner_user_id(Some("user-1"), "").unwrap(), "user-1");
        assert_eq!(
            resolve_owner_user_id(Some("user-1"), "user-1").unwrap(),
            "user-1"
        );
        assert_eq!(resolve_owner_user_id(None, "user-1").unwrap(), "user-1");

        let mismatch = resolve_owner_user_id(Some("user-1"), "user-2").unwrap_err();
        assert!(matches!(mismatch, HarpeError::PermissionDenied(_)));

        let missing = resolve_owner_user_id(None, "").unwrap_err();
        assert!(matches!(missing, HarpeError::PermissionDenied(_)));
    }

    #[test]
    fn preview_context_response_includes_token_estimates() {
        let response = preview_context_to_pb(ChatRequest {
            messages: vec![
                ChatMessage {
                    role: MessageRole::System,
                    content: "Trusted state.".to_owned(),
                },
                ChatMessage {
                    role: MessageRole::User,
                    content: "I open the gate.".to_owned(),
                },
            ],
        });

        assert_eq!(response.messages.len(), 2);
        assert!(response.estimated_tokens >= response.messages[0].estimated_tokens);
        assert_eq!(response.messages[1].role, pb::MessageRole::User as i32);
    }

    #[test]
    fn domain_errors_map_to_expected_grpc_status_codes() {
        let cases = [
            (
                HarpeError::Validation("content".to_owned()),
                tonic::Code::InvalidArgument,
            ),
            (
                HarpeError::NotFound("summary".to_owned()),
                tonic::Code::NotFound,
            ),
            (
                HarpeError::PermissionDenied("game-1".to_owned()),
                tonic::Code::PermissionDenied,
            ),
            (
                HarpeError::Store("database".to_owned()),
                tonic::Code::Internal,
            ),
            (
                HarpeError::Llm("model".to_owned()),
                tonic::Code::Unavailable,
            ),
        ];

        for (error, code) in cases {
            assert_eq!(status_from_error(error).code(), code);
        }
    }

    #[test]
    fn health_service_name_defaults_when_blank() {
        assert_eq!(normalize_health_service(""), "harpe.v1");
        assert_eq!(
            normalize_health_service(" harpe.v1.MemoryService "),
            "harpe.v1.MemoryService"
        );
    }

    #[test]
    fn page_request_overrides_legacy_limit_and_reports_counts() {
        assert_eq!(request_limit(25, None), 25);
        assert_eq!(
            request_limit(
                25,
                Some(&pb::PageRequest {
                    page_size: 3,
                    page_token: String::new(),
                }),
            ),
            3
        );
        assert_eq!(
            request_limit(
                25,
                Some(&pb::PageRequest {
                    page_size: 0,
                    page_token: "ignored-for-now".to_owned(),
                }),
            ),
            25
        );

        let mut items = vec![1, 2, 3, 4];
        truncate_to_limit(&mut items, 2);
        assert_eq!(items, vec![1, 2]);
        assert_eq!(page_info(items.len()).returned_count, 2);
    }

    #[test]
    fn admin_job_status_filter_maps_proto_statuses() {
        assert_eq!(
            admin_job_status_filter(pb::AdminJobStatus::Unspecified as i32).unwrap(),
            None
        );
        assert_eq!(
            admin_job_status_filter(pb::AdminJobStatus::Pending as i32).unwrap(),
            Some(JobStatus::Pending)
        );
        assert_eq!(
            admin_job_status_filter(pb::AdminJobStatus::Running as i32).unwrap(),
            Some(JobStatus::Running)
        );
        assert_eq!(
            admin_job_status_filter(pb::AdminJobStatus::Succeeded as i32).unwrap(),
            Some(JobStatus::Succeeded)
        );
        assert_eq!(
            admin_job_status_filter(pb::AdminJobStatus::Failed as i32).unwrap(),
            Some(JobStatus::Failed)
        );
        assert!(matches!(
            admin_job_status_filter(99),
            Err(HarpeError::Validation(_))
        ));
    }

    #[test]
    fn admin_job_statuses_map_to_proto_values() {
        let cases = [
            (JobStatus::Pending, pb::AdminJobStatus::Pending),
            (JobStatus::Running, pb::AdminJobStatus::Running),
            (JobStatus::Succeeded, pb::AdminJobStatus::Succeeded),
            (JobStatus::Failed, pb::AdminJobStatus::Failed),
        ];

        for (status, proto_status) in cases {
            assert_eq!(admin_job_status_to_pb(status), proto_status as i32);
        }
    }

    #[test]
    fn metrics_snapshot_maps_to_proto() {
        let metrics = AppMetrics::default();
        metrics.record_grpc_request();
        metrics.record_job_retried();

        let response = metrics_to_pb(metrics.snapshot());

        assert_eq!(response.grpc_requests, 1);
        assert_eq!(response.jobs_retried, 1);
        assert!(!response.collected_at.is_empty());
    }
}
