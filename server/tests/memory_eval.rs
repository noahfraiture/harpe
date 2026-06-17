use std::sync::atomic::{AtomicUsize, Ordering};

use async_trait::async_trait;
use harpe_server::db::surreal::SurrealStore;
use harpe_server::domain::{
    MemoryExtraction, MessageRole, NewGame, NewMessage, NewSession, NewUser,
};
use harpe_server::jobs::update_memory_after_turn;
use harpe_server::llm::{
    ChatRequest, ExtractMemoryRequest, LlmClient, SummarizeRequest, TextStream,
};
use harpe_server::store::HarpeStore;
use serde::Deserialize;
use tokio_stream::iter;
use uuid::Uuid;

#[tokio::test]
async fn scripted_transcripts_preserve_expected_story_memory() {
    for (name, json) in [
        ("harbor", include_str!("fixtures/harbor_memory_eval.json")),
        ("social", include_str!("fixtures/social_memory_eval.json")),
        ("combat", include_str!("fixtures/combat_memory_eval.json")),
        (
            "inventory",
            include_str!("fixtures/inventory_memory_eval.json"),
        ),
        ("travel", include_str!("fixtures/travel_memory_eval.json")),
    ] {
        let fixture: MemoryEvalFixture = serde_json::from_str(json).unwrap();
        run_memory_eval(name, fixture).await;
    }
}

async fn run_memory_eval(name: &str, fixture: MemoryEvalFixture) {
    let store = test_store().await;
    let user = store
        .create_user(NewUser {
            display_name: "Eval runner".to_owned(),
        })
        .await
        .unwrap();
    let game = store
        .create_game(NewGame {
            owner_user_id: user.id,
            title: fixture.game_title.clone(),
            system_prompt: fixture.system_prompt.clone(),
        })
        .await
        .unwrap();
    let session = store
        .create_session(NewSession {
            game_id: game.id.clone(),
            title: format!("{name} memory eval"),
        })
        .await
        .unwrap();
    let llm = ScriptedMemoryEvalLlm::new(fixture.turns.clone());

    for turn in &fixture.turns {
        store
            .append_message(NewMessage {
                id: None,
                session_id: session.id.clone(),
                role: MessageRole::User,
                content: turn.user.clone(),
            })
            .await
            .unwrap();
        store
            .append_message(NewMessage {
                id: None,
                session_id: session.id.clone(),
                role: MessageRole::Assistant,
                content: turn.assistant.clone(),
            })
            .await
            .unwrap();

        update_memory_after_turn(&session, &game.id, &turn.assistant, &store, &llm)
            .await
            .unwrap();
    }

    let summary = store.get_story_summary(&session.id).await.unwrap().unwrap();
    for expected in &fixture.expected.summary_contains {
        assert!(
            summary.content.contains(expected),
            "{name} summary did not contain {expected:?}: {}",
            summary.content
        );
    }

    let characters = store.list_characters(&game.id).await.unwrap();
    for (character_name, status) in &fixture.expected.character_status {
        let character = characters
            .iter()
            .find(|character| character.name == *character_name)
            .unwrap_or_else(|| panic!("{name}: missing character {character_name}"));
        assert_eq!(&character.status, status);
    }

    let events = store.list_events(&session.id, 10).await.unwrap();
    let event_summaries = events
        .iter()
        .map(|event| event.summary.as_str())
        .collect::<Vec<_>>();
    for expected in &fixture.expected.event_summaries {
        assert!(
            event_summaries.contains(&expected.as_str()),
            "{name}: missing event {expected:?}; got {event_summaries:?}"
        );
    }

    let facts = store.list_world_facts(&game.id, 10).await.unwrap();
    let fact_contents = facts
        .iter()
        .map(|fact| fact.content.as_str())
        .collect::<Vec<_>>();
    for expected in &fixture.expected.world_fact_contents {
        assert!(
            fact_contents.contains(&expected.as_str()),
            "{name}: missing world fact {expected:?}; got {fact_contents:?}"
        );
    }

    let locations = store.list_locations(&game.id).await.unwrap();
    let location_names = locations
        .iter()
        .map(|location| location.name.as_str())
        .collect::<Vec<_>>();
    for expected in &fixture.expected.locations {
        assert!(
            location_names.contains(&expected.as_str()),
            "{name}: missing location {expected:?}; got {location_names:?}"
        );
    }

    let chunks = store.list_memory_chunks(&session.id, 100).await.unwrap();
    assert!(chunks.iter().any(|chunk| chunk.kind == "turn"));
    assert!(chunks.iter().any(|chunk| chunk.kind == "event"));
    assert!(chunks.iter().any(|chunk| chunk.kind == "character"));
    assert!(chunks.iter().any(|chunk| chunk.kind == "world_fact"));
    assert!(chunks.iter().any(|chunk| chunk.kind == "location"));

    let hits = store
        .search_memory(
            &session.id,
            &fixture.expected.search_query,
            &stable_eval_embedding(&fixture.expected.search_query),
            5,
        )
        .await
        .unwrap();
    assert!(
        hits.iter().any(|hit| hit
            .chunk
            .content
            .contains(&fixture.expected.search_result_contains)),
        "{name}: search hits did not include expected memory: {hits:?}"
    );
}

async fn test_store() -> SurrealStore {
    SurrealStore::connect(
        "memory",
        &format!("memory_eval_{}", Uuid::now_v7()),
        "harpe",
    )
    .await
    .unwrap()
}

#[derive(Clone, Debug, Deserialize)]
struct MemoryEvalFixture {
    game_title: String,
    system_prompt: String,
    turns: Vec<MemoryEvalTurn>,
    expected: ExpectedMemory,
}

#[derive(Clone, Debug, Deserialize)]
struct MemoryEvalTurn {
    user: String,
    assistant: String,
    summary: String,
    extraction: MemoryExtraction,
}

#[derive(Clone, Debug, Deserialize)]
struct ExpectedMemory {
    summary_contains: Vec<String>,
    character_status: std::collections::BTreeMap<String, String>,
    event_summaries: Vec<String>,
    world_fact_contents: Vec<String>,
    locations: Vec<String>,
    search_query: String,
    search_result_contains: String,
}

struct ScriptedMemoryEvalLlm {
    turns: Vec<MemoryEvalTurn>,
    summarize_index: AtomicUsize,
    extraction_index: AtomicUsize,
}

impl ScriptedMemoryEvalLlm {
    fn new(turns: Vec<MemoryEvalTurn>) -> Self {
        Self {
            turns,
            summarize_index: AtomicUsize::new(0),
            extraction_index: AtomicUsize::new(0),
        }
    }
}

#[async_trait]
impl LlmClient for ScriptedMemoryEvalLlm {
    async fn stream_chat(&self, _request: ChatRequest) -> harpe_server::Result<TextStream> {
        Ok(Box::pin(iter(Vec::<harpe_server::Result<String>>::new())))
    }

    async fn summarize(&self, _request: SummarizeRequest) -> harpe_server::Result<String> {
        let index = self.summarize_index.fetch_add(1, Ordering::SeqCst);
        self.turns
            .get(index)
            .map(|turn| turn.summary.clone())
            .ok_or_else(|| harpe_server::HarpeError::Llm(format!("missing summary {index}")))
    }

    async fn extract_memory(
        &self,
        _request: ExtractMemoryRequest,
    ) -> harpe_server::Result<MemoryExtraction> {
        let index = self.extraction_index.fetch_add(1, Ordering::SeqCst);
        self.turns
            .get(index)
            .map(|turn| turn.extraction.clone())
            .ok_or_else(|| harpe_server::HarpeError::Llm(format!("missing extraction {index}")))
    }

    async fn embed(&self, text: &str) -> harpe_server::Result<Vec<f32>> {
        Ok(stable_eval_embedding(text))
    }
}

fn stable_eval_embedding(text: &str) -> Vec<f32> {
    let mut embedding = vec![0.0; 16];
    for (index, byte) in text.bytes().enumerate() {
        embedding[index % 16] += f32::from(byte) / 255.0;
    }

    let norm = embedding
        .iter()
        .map(|value| value * value)
        .sum::<f32>()
        .sqrt();
    if norm > 0.0 {
        for value in &mut embedding {
            *value /= norm;
        }
    }

    embedding
}
