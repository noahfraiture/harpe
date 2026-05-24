use std::sync::Arc;

use futures_util::StreamExt;
use harpe_server::api::grpc::HarpeGrpc;
use harpe_server::db::surreal::SurrealStore;
use harpe_server::domain::{
    MessageRole, NewGame, NewMemoryChunk, NewMessage, NewSession, UpsertCharacter,
    UpsertStorySummary,
};
use harpe_server::llm::EchoLlm;
use harpe_server::pb::game_service_client::GameServiceClient;
use harpe_server::pb::game_service_server::GameServiceServer;
use harpe_server::pb::memory_service_client::MemoryServiceClient;
use harpe_server::pb::memory_service_server::MemoryServiceServer;
use harpe_server::pb::session_service_client::SessionServiceClient;
use harpe_server::pb::session_service_server::SessionServiceServer;
use harpe_server::pb::{
    CreateGameRequest, CreateSessionRequest, GetStorySummaryRequest, ListMessagesRequest,
    SendMessageRequest,
};
use harpe_server::store::HarpeStore;
use tokio::net::TcpListener;
use tokio_stream::wrappers::TcpListenerStream;
use tonic::transport::{Endpoint, Server};
use uuid::Uuid;

#[tokio::test]
async fn surreal_store_round_trips_conversation_memory_and_characters() {
    let store = test_store().await;
    store.migrate().await.unwrap();

    let game = store
        .create_game(NewGame {
            title: "Vaults of Glass".to_owned(),
            system_prompt: "Run a tense fantasy mystery.".to_owned(),
        })
        .await
        .unwrap();
    let session = store
        .create_session(NewSession {
            game_id: game.id.clone(),
            title: "Session 1".to_owned(),
        })
        .await
        .unwrap();

    store
        .append_message(NewMessage {
            id: None,
            session_id: session.id.clone(),
            role: MessageRole::User,
            content: "I inspect the silver door.".to_owned(),
        })
        .await
        .unwrap();
    store
        .append_message(NewMessage {
            id: None,
            session_id: session.id.clone(),
            role: MessageRole::Assistant,
            content: "A keyhole glows beneath the dust.".to_owned(),
        })
        .await
        .unwrap();

    let messages = store.list_recent_messages(&session.id, 10).await.unwrap();
    assert_eq!(messages.len(), 2);
    assert_eq!(messages[0].role, MessageRole::User);
    assert_eq!(messages[1].role, MessageRole::Assistant);

    store
        .upsert_story_summary(UpsertStorySummary {
            session_id: session.id.clone(),
            content: "The party found a sealed silver door.".to_owned(),
        })
        .await
        .unwrap();
    let summary = store.get_story_summary(&session.id).await.unwrap().unwrap();
    assert!(summary.content.contains("silver door"));

    let character = store
        .upsert_character(UpsertCharacter {
            id: None,
            game_id: game.id.clone(),
            name: "Mira".to_owned(),
            description: "Archivist and reluctant guide".to_owned(),
            status: "nervous".to_owned(),
        })
        .await
        .unwrap();
    let characters = store.list_characters(&game.id).await.unwrap();
    assert_eq!(characters, vec![character]);

    let saved = store
        .save_memory_chunk(NewMemoryChunk {
            session_id: session.id.clone(),
            kind: "event".to_owned(),
            content: "The silver key opens the lower vault.".to_owned(),
            embedding: vec![1.0, 0.0],
        })
        .await
        .unwrap();
    let hits = store
        .search_memory(&session.id, "silver key", &[1.0, 0.0], 5)
        .await
        .unwrap();

    assert_eq!(hits[0].chunk.id, saved.chunk.id);
    assert!(hits[0].score > 0.99);
}

#[tokio::test]
async fn grpc_send_message_streams_response_and_updates_memory() {
    let store = Arc::new(test_store().await);
    let llm = Arc::new(EchoLlm::new(vec![
        "The gate ".to_owned(),
        "opens.".to_owned(),
    ]));
    let service = HarpeGrpc::new(store, llm);
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    let server = tokio::spawn(
        Server::builder()
            .add_service(GameServiceServer::new(service.clone()))
            .add_service(SessionServiceServer::new(service.clone()))
            .add_service(MemoryServiceServer::new(service))
            .serve_with_incoming(TcpListenerStream::new(listener)),
    );

    let channel = Endpoint::from_shared(format!("http://{addr}"))
        .unwrap()
        .connect()
        .await
        .unwrap();

    let mut game_client = GameServiceClient::new(channel.clone());
    let game = game_client
        .create_game(CreateGameRequest {
            title: "Iron Coast".to_owned(),
            system_prompt: "Run a coastal fantasy adventure.".to_owned(),
        })
        .await
        .unwrap()
        .into_inner();

    let mut session_client = SessionServiceClient::new(channel.clone());
    let session = session_client
        .create_session(CreateSessionRequest {
            game_id: game.id,
            title: "First watch".to_owned(),
        })
        .await
        .unwrap()
        .into_inner();

    let mut stream = session_client
        .send_message(SendMessageRequest {
            session_id: session.id.clone(),
            content: "I lift the rusted latch.".to_owned(),
        })
        .await
        .unwrap()
        .into_inner();

    let mut response = String::new();
    let mut saw_done = false;
    while let Some(delta) = stream.next().await {
        let delta = delta.unwrap();
        response.push_str(&delta.delta);
        saw_done = delta.done;
        if saw_done {
            break;
        }
    }

    assert_eq!(response, "The gate opens.");
    assert!(saw_done);

    let messages = session_client
        .list_messages(ListMessagesRequest {
            session_id: session.id.clone(),
            limit: 10,
        })
        .await
        .unwrap()
        .into_inner()
        .messages;
    assert_eq!(messages.len(), 2);

    let mut memory_client = MemoryServiceClient::new(channel);
    let summary = memory_client
        .get_story_summary(GetStorySummaryRequest {
            session_id: session.id,
        })
        .await
        .unwrap()
        .into_inner();
    assert!(summary.content.contains("The gate opens."));

    server.abort();
}

async fn test_store() -> SurrealStore {
    SurrealStore::connect("memory", &format!("test_{}", Uuid::now_v7()), "harpe")
        .await
        .unwrap()
}
