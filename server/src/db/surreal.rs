use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use surrealdb::Surreal;
use surrealdb::engine::any::{self, Any};
use surrealdb::types::SurrealValue;

use crate::domain::{
    Character, Game, MemoryChunk, MemoryHit, Message, MessageRole, NewGame, NewMemoryChunk,
    NewMessage, NewSession, Session, StorySummary, UpsertCharacter, UpsertStorySummary, new_id,
};
use crate::engine::cosine_similarity;
use crate::store::HarpeStore;
use crate::{HarpeError, Result};

const MIGRATIONS: &str = r#"
DEFINE TABLE game SCHEMALESS;
DEFINE TABLE session SCHEMALESS;
DEFINE TABLE message SCHEMALESS;
DEFINE TABLE summary SCHEMALESS;
DEFINE TABLE character SCHEMALESS;
DEFINE TABLE memory_chunk SCHEMALESS;
DEFINE INDEX game_created_at ON game FIELDS created_at;
DEFINE INDEX session_game_id ON session FIELDS game_id;
DEFINE INDEX message_session_id ON message FIELDS session_id;
DEFINE INDEX character_game_id ON character FIELDS game_id;
DEFINE INDEX memory_chunk_session_id ON memory_chunk FIELDS session_id;
"#;

#[derive(Clone)]
pub struct SurrealStore {
    db: Arc<Surreal<Any>>,
}

impl SurrealStore {
    pub async fn connect(
        endpoint: impl Into<String>,
        namespace: &str,
        database: &str,
    ) -> Result<Self> {
        let db = any::connect(endpoint.into()).await?;
        db.use_ns(namespace).use_db(database).await?;

        let store = Self { db: Arc::new(db) };
        store.migrate().await?;

        Ok(store)
    }

    pub async fn migrate(&self) -> Result<()> {
        self.db.query(MIGRATIONS).await?;
        Ok(())
    }

    pub fn db(&self) -> &Surreal<Any> {
        &self.db
    }
}

#[async_trait]
impl HarpeStore for SurrealStore {
    async fn create_game(&self, input: NewGame) -> Result<Game> {
        validate_present("game title", &input.title)?;

        let row = GameRow {
            uid: new_id(),
            title: input.title,
            system_prompt: input.system_prompt,
            created_at: Utc::now(),
        };

        let created: Option<GameRow> = self
            .db
            .create(("game", row.uid.as_str()))
            .content(row)
            .await?;

        created
            .map(Into::into)
            .ok_or_else(|| HarpeError::Store("SurrealDB did not return created game".to_owned()))
    }

    async fn list_games(&self, limit: usize) -> Result<Vec<Game>> {
        let mut response = self
            .db
            .query("SELECT * FROM game ORDER BY created_at DESC LIMIT $limit")
            .bind(("limit", normalize_limit(limit) as i64))
            .await?;
        let rows: Vec<GameRow> = response.take(0)?;

        Ok(rows.into_iter().map(Into::into).collect())
    }

    async fn get_game(&self, game_id: &str) -> Result<Game> {
        let row: Option<GameRow> = self.db.select(("game", game_id)).await?;
        row.map(Into::into)
            .ok_or_else(|| HarpeError::NotFound(format!("game {game_id}")))
    }

    async fn create_session(&self, input: NewSession) -> Result<Session> {
        validate_present("game id", &input.game_id)?;
        validate_present("session title", &input.title)?;
        self.get_game(&input.game_id).await?;

        let row = SessionRow {
            uid: new_id(),
            game_id: input.game_id,
            title: input.title,
            created_at: Utc::now(),
        };

        let created: Option<SessionRow> = self
            .db
            .create(("session", row.uid.as_str()))
            .content(row)
            .await?;

        created
            .map(Into::into)
            .ok_or_else(|| HarpeError::Store("SurrealDB did not return created session".to_owned()))
    }

    async fn get_session(&self, session_id: &str) -> Result<Session> {
        let row: Option<SessionRow> = self.db.select(("session", session_id)).await?;
        row.map(Into::into)
            .ok_or_else(|| HarpeError::NotFound(format!("session {session_id}")))
    }

    async fn append_message(&self, input: NewMessage) -> Result<Message> {
        validate_present("session id", &input.session_id)?;
        validate_present("message content", &input.content)?;

        let row = MessageRow {
            uid: input.id.unwrap_or_else(new_id),
            session_id: input.session_id,
            role: input.role.as_db_value().to_owned(),
            content: input.content,
            created_at: Utc::now(),
        };

        let created: Option<MessageRow> = self
            .db
            .create(("message", row.uid.as_str()))
            .content(row)
            .await?;

        created
            .map(TryInto::try_into)
            .transpose()?
            .ok_or_else(|| HarpeError::Store("SurrealDB did not return created message".to_owned()))
    }

    async fn list_recent_messages(&self, session_id: &str, limit: usize) -> Result<Vec<Message>> {
        let mut response = self
            .db
            .query(
                "SELECT * FROM message WHERE session_id = $session_id ORDER BY created_at DESC LIMIT $limit",
            )
            .bind(("session_id", session_id.to_owned()))
            .bind(("limit", normalize_limit(limit) as i64))
            .await?;
        let mut rows: Vec<MessageRow> = response.take(0)?;
        rows.reverse();

        rows.into_iter().map(TryInto::try_into).collect()
    }

    async fn get_story_summary(&self, session_id: &str) -> Result<Option<StorySummary>> {
        let row: Option<SummaryRow> = self.db.select(("summary", session_id)).await?;
        Ok(row.map(Into::into))
    }

    async fn upsert_story_summary(&self, input: UpsertStorySummary) -> Result<StorySummary> {
        validate_present("session id", &input.session_id)?;

        let row = SummaryRow {
            uid: input.session_id.clone(),
            session_id: input.session_id,
            content: input.content,
            updated_at: Utc::now(),
        };

        let upserted: Option<SummaryRow> = self
            .db
            .upsert(("summary", row.uid.as_str()))
            .content(row)
            .await?;

        upserted.map(Into::into).ok_or_else(|| {
            HarpeError::Store("SurrealDB did not return upserted summary".to_owned())
        })
    }

    async fn upsert_character(&self, input: UpsertCharacter) -> Result<Character> {
        validate_present("game id", &input.game_id)?;
        validate_present("character name", &input.name)?;

        let uid = input.id.unwrap_or_else(new_id);
        let row = CharacterRow {
            uid,
            game_id: input.game_id,
            name: input.name,
            description: input.description,
            status: input.status,
            updated_at: Utc::now(),
        };

        let upserted: Option<CharacterRow> = self
            .db
            .upsert(("character", row.uid.as_str()))
            .content(row)
            .await?;

        upserted.map(Into::into).ok_or_else(|| {
            HarpeError::Store("SurrealDB did not return upserted character".to_owned())
        })
    }

    async fn list_characters(&self, game_id: &str) -> Result<Vec<Character>> {
        let mut response = self
            .db
            .query("SELECT * FROM character WHERE game_id = $game_id ORDER BY name ASC")
            .bind(("game_id", game_id.to_owned()))
            .await?;
        let rows: Vec<CharacterRow> = response.take(0)?;

        Ok(rows.into_iter().map(Into::into).collect())
    }

    async fn get_character(&self, character_id: &str) -> Result<Character> {
        let row: Option<CharacterRow> = self.db.select(("character", character_id)).await?;
        row.map(Into::into)
            .ok_or_else(|| HarpeError::NotFound(format!("character {character_id}")))
    }

    async fn save_memory_chunk(&self, input: NewMemoryChunk) -> Result<MemoryHit> {
        validate_present("session id", &input.session_id)?;
        validate_present("memory content", &input.content)?;

        let row = MemoryChunkRow {
            uid: new_id(),
            session_id: input.session_id,
            kind: input.kind,
            content: input.content,
            embedding: input.embedding,
            created_at: Utc::now(),
        };

        let created: Option<MemoryChunkRow> = self
            .db
            .create(("memory_chunk", row.uid.as_str()))
            .content(row)
            .await?;

        created
            .map(|row| MemoryHit {
                chunk: row.into(),
                score: 1.0,
            })
            .ok_or_else(|| {
                HarpeError::Store("SurrealDB did not return created memory chunk".to_owned())
            })
    }

    async fn search_memory(
        &self,
        session_id: &str,
        query: &str,
        query_embedding: &[f32],
        limit: usize,
    ) -> Result<Vec<MemoryHit>> {
        let mut response = self
            .db
            .query(
                "SELECT * FROM memory_chunk WHERE session_id = $session_id ORDER BY created_at DESC LIMIT 200",
            )
            .bind(("session_id", session_id.to_owned()))
            .await?;
        let rows: Vec<MemoryChunkRow> = response.take(0)?;
        let query = query.to_lowercase();

        let mut hits = rows
            .into_iter()
            .map(|row| {
                let vector_score = cosine_similarity(query_embedding, &row.embedding);
                let lexical_score = lexical_score(&query, &row.content);
                MemoryHit {
                    chunk: row.into(),
                    score: vector_score.max(lexical_score),
                }
            })
            .filter(|hit| hit.score > 0.0 || query.is_empty())
            .collect::<Vec<_>>();

        hits.sort_by(|left, right| {
            right
                .score
                .total_cmp(&left.score)
                .then_with(|| right.chunk.created_at.cmp(&left.chunk.created_at))
        });
        hits.truncate(normalize_limit(limit));

        Ok(hits)
    }
}

fn validate_present(label: &str, value: &str) -> Result<()> {
    if value.trim().is_empty() {
        return Err(HarpeError::Validation(format!("{label} is required")));
    }

    Ok(())
}

fn normalize_limit(limit: usize) -> usize {
    match limit {
        0 => 50,
        1..=100 => limit,
        _ => 100,
    }
}

fn lexical_score(query: &str, content: &str) -> f32 {
    if query.is_empty() {
        return 0.0;
    }

    let content = content.to_lowercase();
    let terms = query
        .split_whitespace()
        .filter(|term| !term.is_empty())
        .collect::<Vec<_>>();

    if terms.is_empty() {
        return 0.0;
    }

    let matches = terms.iter().filter(|term| content.contains(**term)).count();

    matches as f32 / terms.len() as f32
}

#[derive(Clone, Debug, Serialize, Deserialize, SurrealValue)]
struct GameRow {
    uid: String,
    title: String,
    system_prompt: String,
    created_at: DateTime<Utc>,
}

impl From<GameRow> for Game {
    fn from(value: GameRow) -> Self {
        Self {
            id: value.uid,
            title: value.title,
            system_prompt: value.system_prompt,
            created_at: value.created_at,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, SurrealValue)]
struct SessionRow {
    uid: String,
    game_id: String,
    title: String,
    created_at: DateTime<Utc>,
}

impl From<SessionRow> for Session {
    fn from(value: SessionRow) -> Self {
        Self {
            id: value.uid,
            game_id: value.game_id,
            title: value.title,
            created_at: value.created_at,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, SurrealValue)]
struct MessageRow {
    uid: String,
    session_id: String,
    role: String,
    content: String,
    created_at: DateTime<Utc>,
}

impl TryFrom<MessageRow> for Message {
    type Error = HarpeError;

    fn try_from(value: MessageRow) -> Result<Self> {
        let role = MessageRole::from_db_value(&value.role)
            .ok_or_else(|| HarpeError::Store(format!("unknown message role {}", value.role)))?;

        Ok(Self {
            id: value.uid,
            session_id: value.session_id,
            role,
            content: value.content,
            created_at: value.created_at,
        })
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, SurrealValue)]
struct SummaryRow {
    uid: String,
    session_id: String,
    content: String,
    updated_at: DateTime<Utc>,
}

impl From<SummaryRow> for StorySummary {
    fn from(value: SummaryRow) -> Self {
        Self {
            session_id: value.session_id,
            content: value.content,
            updated_at: value.updated_at,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, SurrealValue)]
struct CharacterRow {
    uid: String,
    game_id: String,
    name: String,
    description: String,
    status: String,
    updated_at: DateTime<Utc>,
}

impl From<CharacterRow> for Character {
    fn from(value: CharacterRow) -> Self {
        Self {
            id: value.uid,
            game_id: value.game_id,
            name: value.name,
            description: value.description,
            status: value.status,
            updated_at: value.updated_at,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, SurrealValue)]
struct MemoryChunkRow {
    uid: String,
    session_id: String,
    kind: String,
    content: String,
    embedding: Vec<f32>,
    created_at: DateTime<Utc>,
}

impl From<MemoryChunkRow> for MemoryChunk {
    fn from(value: MemoryChunkRow) -> Self {
        Self {
            id: value.uid,
            session_id: value.session_id,
            kind: value.kind,
            content: value.content,
            embedding: value.embedding,
            created_at: value.created_at,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lexical_score_counts_query_terms() {
        assert_eq!(
            lexical_score("silver key", "The silver door is locked."),
            0.5
        );
        assert_eq!(lexical_score("silver key", "The silver key turns."), 1.0);
        assert_eq!(lexical_score("", "anything"), 0.0);
    }

    #[test]
    fn limit_defaults_and_caps() {
        assert_eq!(normalize_limit(0), 50);
        assert_eq!(normalize_limit(10), 10);
        assert_eq!(normalize_limit(1_000), 100);
    }
}
