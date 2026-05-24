use async_trait::async_trait;

use crate::Result;
use crate::domain::{
    Character, Event, Game, Location, MemoryHit, Message, NewEvent, NewGame, NewMemoryChunk,
    NewMessage, NewSession, Session, StorySummary, UpsertCharacter, UpsertLocation,
    UpsertStorySummary, UpsertWorldFact, WorldFact,
};

#[async_trait]
pub trait HarpeStore: Send + Sync {
    async fn create_game(&self, input: NewGame) -> Result<Game>;
    async fn list_games(&self, limit: usize) -> Result<Vec<Game>>;
    async fn get_game(&self, game_id: &str) -> Result<Game>;

    async fn create_session(&self, input: NewSession) -> Result<Session>;
    async fn get_session(&self, session_id: &str) -> Result<Session>;

    async fn append_message(&self, input: NewMessage) -> Result<Message>;
    async fn list_recent_messages(&self, session_id: &str, limit: usize) -> Result<Vec<Message>>;

    async fn get_story_summary(&self, session_id: &str) -> Result<Option<StorySummary>>;
    async fn upsert_story_summary(&self, input: UpsertStorySummary) -> Result<StorySummary>;

    async fn upsert_character(&self, input: UpsertCharacter) -> Result<Character>;
    async fn list_characters(&self, game_id: &str) -> Result<Vec<Character>>;
    async fn get_character(&self, character_id: &str) -> Result<Character>;

    async fn save_event(&self, input: NewEvent) -> Result<Event>;
    async fn list_events(&self, session_id: &str, limit: usize) -> Result<Vec<Event>>;

    async fn upsert_location(&self, input: UpsertLocation) -> Result<Location>;
    async fn list_locations(&self, game_id: &str) -> Result<Vec<Location>>;

    async fn upsert_world_fact(&self, input: UpsertWorldFact) -> Result<WorldFact>;
    async fn list_world_facts(&self, game_id: &str, limit: usize) -> Result<Vec<WorldFact>>;

    async fn save_memory_chunk(&self, input: NewMemoryChunk) -> Result<MemoryHit>;
    async fn search_memory(
        &self,
        session_id: &str,
        query: &str,
        query_embedding: &[f32],
        limit: usize,
    ) -> Result<Vec<MemoryHit>>;
}
