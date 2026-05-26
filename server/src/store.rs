use async_trait::async_trait;

use crate::Result;
use crate::domain::{
    BackgroundJob, Character, Event, Game, GameSnapshot, GraphEdge, GraphRelationKind, JobStatus,
    Location, MemoryChunk, MemoryHit, Message, NewBackgroundJob, NewEvent, NewGame, NewMemoryChunk,
    NewMessage, NewSession, NewUser, Session, StorySummary, UpsertCharacter, UpsertLocation,
    UpsertStorySummary, UpsertWorldFact, User, WorldFact,
};

#[async_trait]
pub trait HarpeStore: Send + Sync {
    async fn create_user(&self, input: NewUser) -> Result<User>;
    async fn get_user(&self, user_id: &str) -> Result<User>;

    async fn create_game(&self, input: NewGame) -> Result<Game>;
    async fn list_games(&self, limit: usize) -> Result<Vec<Game>>;
    async fn list_games_for_user(&self, owner_user_id: &str, limit: usize) -> Result<Vec<Game>>;
    async fn get_game(&self, game_id: &str) -> Result<Game>;
    async fn export_game_snapshot(&self, game_id: &str) -> Result<GameSnapshot> {
        let game = self.get_game(game_id).await?;
        let sessions = self.list_sessions(game_id, 1_000).await?;
        let mut summaries = Vec::new();
        let mut events = Vec::new();
        let mut memory_chunks = Vec::new();

        for session in &sessions {
            if let Some(summary) = self.get_story_summary(&session.id).await? {
                summaries.push(summary);
            }
            events.extend(self.list_events(&session.id, 1_000).await?);
            memory_chunks.extend(self.list_memory_chunks(&session.id, 1_000).await?);
        }

        Ok(GameSnapshot {
            characters: self.list_characters(game_id).await?,
            world_facts: self.list_world_facts(game_id, 1_000).await?,
            locations: self.list_locations(game_id).await?,
            game,
            sessions,
            summaries,
            events,
            memory_chunks,
            exported_at: chrono::Utc::now(),
        })
    }

    async fn enqueue_job(&self, input: NewBackgroundJob) -> Result<BackgroundJob>;
    async fn list_jobs(
        &self,
        status: Option<JobStatus>,
        limit: usize,
    ) -> Result<Vec<BackgroundJob>>;
    async fn claim_next_job(&self) -> Result<Option<BackgroundJob>>;
    async fn complete_job(&self, job_id: &str) -> Result<BackgroundJob>;
    async fn retry_job(
        &self,
        job_id: &str,
        error: String,
        run_after: chrono::DateTime<chrono::Utc>,
    ) -> Result<BackgroundJob>;
    async fn fail_job(&self, job_id: &str, error: String) -> Result<BackgroundJob>;

    async fn create_session(&self, input: NewSession) -> Result<Session>;
    async fn list_sessions(&self, game_id: &str, limit: usize) -> Result<Vec<Session>>;
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

    async fn list_graph_edges(
        &self,
        relation: GraphRelationKind,
        in_record_id: &str,
    ) -> Result<Vec<GraphEdge>>;

    async fn save_memory_chunk(&self, input: NewMemoryChunk) -> Result<MemoryHit>;
    async fn list_memory_chunks(&self, session_id: &str, limit: usize) -> Result<Vec<MemoryChunk>>;
    async fn search_memory(
        &self,
        session_id: &str,
        query: &str,
        query_embedding: &[f32],
        limit: usize,
    ) -> Result<Vec<MemoryHit>>;
}
