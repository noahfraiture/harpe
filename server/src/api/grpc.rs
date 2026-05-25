use std::sync::Arc;

use futures_util::StreamExt;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tonic::{Request, Response, Status};

use crate::domain::{
    Character, Event, Game, Location, MemoryHit, Message, MessageRole, NewGame, NewMessage,
    NewSession, Session, StorySummary, WorldFact, new_id,
};
use crate::engine::{ContextBuilder, ContextInputs};
use crate::jobs::{UpdateMemoryAfterTurnPayload, new_update_memory_job};
use crate::llm::LlmClient;
use crate::pb::{
    self, CreateGameRequest, CreateSessionRequest, GetCharacterRequest, GetGameRequest,
    GetSessionRequest, GetStorySummaryRequest, ListCharactersRequest, ListEventsRequest,
    ListGamesRequest, ListLocationsRequest, ListMessagesRequest, ListWorldFactsRequest,
    MessageDelta, SearchMemoryRequest, SendMessageRequest, game_service_server,
    memory_service_server, session_service_server,
};
use crate::store::HarpeStore;
use crate::{HarpeError, Result};

#[derive(Clone)]
pub struct HarpeGrpc {
    store: Arc<dyn HarpeStore>,
    llm: Arc<dyn LlmClient>,
    context_builder: ContextBuilder,
}

impl HarpeGrpc {
    pub fn new(store: Arc<dyn HarpeStore>, llm: Arc<dyn LlmClient>) -> Self {
        Self {
            store,
            llm,
            context_builder: ContextBuilder::default(),
        }
    }

    pub fn with_context_builder(mut self, context_builder: ContextBuilder) -> Self {
        self.context_builder = context_builder;
        self
    }
}

#[tonic::async_trait]
impl game_service_server::GameService for HarpeGrpc {
    async fn create_game(
        &self,
        request: Request<CreateGameRequest>,
    ) -> std::result::Result<Response<pb::Game>, Status> {
        let request = request.into_inner();
        let game = self
            .store
            .create_game(NewGame {
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
        let games = self
            .store
            .list_games(limit_from_u32(request.into_inner().limit))
            .await
            .map_err(status_from_error)?
            .into_iter()
            .map(game_to_pb)
            .collect();

        Ok(Response::new(pb::ListGamesResponse { games }))
    }

    async fn get_game(
        &self,
        request: Request<GetGameRequest>,
    ) -> std::result::Result<Response<pb::Game>, Status> {
        let game = self
            .store
            .get_game(&request.into_inner().game_id)
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
        let request = request.into_inner();
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

    async fn get_session(
        &self,
        request: Request<GetSessionRequest>,
    ) -> std::result::Result<Response<pb::Session>, Status> {
        let session = self
            .store
            .get_session(&request.into_inner().session_id)
            .await
            .map_err(status_from_error)?;

        Ok(Response::new(session_to_pb(session)))
    }

    async fn send_message(
        &self,
        request: Request<SendMessageRequest>,
    ) -> std::result::Result<Response<Self::SendMessageStream>, Status> {
        let request = request.into_inner();
        let store = Arc::clone(&self.store);
        let llm = Arc::clone(&self.llm);
        let context_builder = self.context_builder.clone();
        let (tx, rx) = mpsc::channel(16);

        tokio::spawn(async move {
            if let Err(error) =
                run_send_message(request, store, llm, context_builder, tx.clone()).await
            {
                let _ = tx.send(Err(status_from_error(error))).await;
            }
        });

        Ok(Response::new(ReceiverStream::new(rx)))
    }

    async fn list_messages(
        &self,
        request: Request<ListMessagesRequest>,
    ) -> std::result::Result<Response<pb::ListMessagesResponse>, Status> {
        let request = request.into_inner();
        let messages = self
            .store
            .list_recent_messages(&request.session_id, limit_from_u32(request.limit))
            .await
            .map_err(status_from_error)?
            .into_iter()
            .map(message_to_pb)
            .collect();

        Ok(Response::new(pb::ListMessagesResponse { messages }))
    }
}

#[tonic::async_trait]
impl memory_service_server::MemoryService for HarpeGrpc {
    async fn get_story_summary(
        &self,
        request: Request<GetStorySummaryRequest>,
    ) -> std::result::Result<Response<pb::StorySummary>, Status> {
        let request = request.into_inner();
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
        let characters = self
            .store
            .list_characters(&request.into_inner().game_id)
            .await
            .map_err(status_from_error)?
            .into_iter()
            .map(character_to_pb)
            .collect();

        Ok(Response::new(pb::ListCharactersResponse { characters }))
    }

    async fn get_character(
        &self,
        request: Request<GetCharacterRequest>,
    ) -> std::result::Result<Response<pb::Character>, Status> {
        let character = self
            .store
            .get_character(&request.into_inner().character_id)
            .await
            .map_err(status_from_error)?;

        Ok(Response::new(character_to_pb(character)))
    }

    async fn list_events(
        &self,
        request: Request<ListEventsRequest>,
    ) -> std::result::Result<Response<pb::ListEventsResponse>, Status> {
        let request = request.into_inner();
        let events = self
            .store
            .list_events(&request.session_id, limit_from_u32(request.limit))
            .await
            .map_err(status_from_error)?
            .into_iter()
            .map(event_to_pb)
            .collect();

        Ok(Response::new(pb::ListEventsResponse { events }))
    }

    async fn list_world_facts(
        &self,
        request: Request<ListWorldFactsRequest>,
    ) -> std::result::Result<Response<pb::ListWorldFactsResponse>, Status> {
        let request = request.into_inner();
        let facts = self
            .store
            .list_world_facts(&request.game_id, limit_from_u32(request.limit))
            .await
            .map_err(status_from_error)?
            .into_iter()
            .map(world_fact_to_pb)
            .collect();

        Ok(Response::new(pb::ListWorldFactsResponse { facts }))
    }

    async fn list_locations(
        &self,
        request: Request<ListLocationsRequest>,
    ) -> std::result::Result<Response<pb::ListLocationsResponse>, Status> {
        let locations = self
            .store
            .list_locations(&request.into_inner().game_id)
            .await
            .map_err(status_from_error)?
            .into_iter()
            .map(location_to_pb)
            .collect();

        Ok(Response::new(pb::ListLocationsResponse { locations }))
    }

    async fn search_memory(
        &self,
        request: Request<SearchMemoryRequest>,
    ) -> std::result::Result<Response<pb::SearchMemoryResponse>, Status> {
        let request = request.into_inner();
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
                limit_from_u32(request.limit),
            )
            .await
            .map_err(status_from_error)?
            .into_iter()
            .map(memory_hit_to_pb)
            .collect();

        Ok(Response::new(pb::SearchMemoryResponse { hits }))
    }
}

async fn run_send_message(
    request: SendMessageRequest,
    store: Arc<dyn HarpeStore>,
    llm: Arc<dyn LlmClient>,
    context_builder: ContextBuilder,
    tx: mpsc::Sender<std::result::Result<MessageDelta, Status>>,
) -> Result<()> {
    if request.content.trim().is_empty() {
        return Err(HarpeError::Validation(
            "message content is required".to_owned(),
        ));
    }

    let session = store.get_session(&request.session_id).await?;
    let game = store.get_game(&session.game_id).await?;

    store
        .append_message(NewMessage {
            id: None,
            session_id: session.id.clone(),
            role: MessageRole::User,
            content: request.content.clone(),
        })
        .await?;

    let query_embedding = llm.embed(&request.content).await?;
    let summary = store.get_story_summary(&session.id).await?;
    let recent_events = store.list_events(&session.id, 12).await?;
    let memories = store
        .search_memory(
            &session.id,
            &request.content,
            &query_embedding,
            context_builder.memory_limit,
        )
        .await?;
    let characters = store.list_characters(&game.id).await?;
    let world_facts = store.list_world_facts(&game.id, 24).await?;
    let locations = store.list_locations(&game.id).await?;
    let recent_messages = store
        .list_recent_messages(&session.id, context_builder.recent_message_limit)
        .await?;
    let chat_request = context_builder.build(ContextInputs {
        base_system_prompt: game.system_prompt,
        summary: summary.clone(),
        recent_events,
        memories,
        characters,
        world_facts,
        locations,
        recent_messages,
    });

    let assistant_id = new_id();
    let mut response_stream = llm.stream_chat(chat_request).await?;
    let mut assistant_content = String::new();

    while let Some(delta) = response_stream.next().await {
        let delta = delta?;
        assistant_content.push_str(&delta);

        if tx
            .send(Ok(MessageDelta {
                session_id: session.id.clone(),
                message_id: assistant_id.clone(),
                delta,
                done: false,
            }))
            .await
            .is_err()
        {
            return Ok(());
        }
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
        }))
        .await;

    Ok(())
}

fn limit_from_u32(limit: u32) -> usize {
    usize::try_from(limit).unwrap_or(usize::MAX)
}

fn status_from_error(error: HarpeError) -> Status {
    match error {
        HarpeError::Validation(message) => Status::invalid_argument(message),
        HarpeError::NotFound(message) => Status::not_found(message),
        HarpeError::Store(message) => Status::internal(message),
        HarpeError::Llm(message) => Status::unavailable(message),
    }
}

fn game_to_pb(game: Game) -> pb::Game {
    pb::Game {
        id: game.id,
        title: game.title,
        system_prompt: game.system_prompt,
        created_at: game.created_at.to_rfc3339(),
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

fn role_to_pb(role: MessageRole) -> i32 {
    match role {
        MessageRole::System => pb::MessageRole::System as i32,
        MessageRole::User => pb::MessageRole::User as i32,
        MessageRole::Assistant => pb::MessageRole::Assistant as i32,
    }
}
