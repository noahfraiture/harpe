use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use surrealdb::types::{RecordId, SurrealValue};

use super::migrations::AppliedMigration;
use crate::domain::{
    BackgroundJob, Character, Event, Game, JobKind, JobStatus, Location, MemoryChunk, Message,
    MessageRole, Session, StorySummary, User, WorldFact,
};
use crate::{HarpeError, Result};

#[derive(Clone, Debug, Serialize, Deserialize, SurrealValue)]
pub(super) struct MigrationRow {
    pub(super) uid: String,
    pub(super) version: i32,
    pub(super) name: String,
    pub(super) applied_at: DateTime<Utc>,
}

impl From<MigrationRow> for AppliedMigration {
    fn from(value: MigrationRow) -> Self {
        Self {
            version: value.version,
            name: value.name,
            applied_at: value.applied_at,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, SurrealValue)]
pub(super) struct UserRow {
    pub(super) uid: String,
    pub(super) display_name: String,
    pub(super) created_at: DateTime<Utc>,
}

impl From<UserRow> for User {
    fn from(value: UserRow) -> Self {
        Self {
            id: value.uid,
            display_name: value.display_name,
            created_at: value.created_at,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, SurrealValue)]
pub(super) struct GameRow {
    pub(super) uid: String,
    pub(super) owner_user_id: String,
    pub(super) title: String,
    pub(super) system_prompt: String,
    pub(super) created_at: DateTime<Utc>,
}

impl From<GameRow> for Game {
    fn from(value: GameRow) -> Self {
        Self {
            id: value.uid,
            owner_user_id: value.owner_user_id,
            title: value.title,
            system_prompt: value.system_prompt,
            created_at: value.created_at,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, SurrealValue)]
pub(super) struct JobRow {
    pub(super) uid: String,
    pub(super) kind: String,
    pub(super) status: String,
    pub(super) payload_json: String,
    pub(super) attempts: i32,
    pub(super) max_attempts: i32,
    pub(super) last_error: Option<String>,
    pub(super) run_after: DateTime<Utc>,
    pub(super) created_at: DateTime<Utc>,
    pub(super) updated_at: DateTime<Utc>,
}

impl TryFrom<JobRow> for BackgroundJob {
    type Error = HarpeError;

    fn try_from(value: JobRow) -> Result<Self> {
        let kind = JobKind::from_db_value(&value.kind)
            .ok_or_else(|| HarpeError::Store(format!("unknown job kind {}", value.kind)))?;
        let status = JobStatus::from_db_value(&value.status)
            .ok_or_else(|| HarpeError::Store(format!("unknown job status {}", value.status)))?;
        let payload = serde_json::from_str(&value.payload_json)
            .map_err(|error| HarpeError::Store(error.to_string()))?;

        Ok(Self {
            id: value.uid,
            kind,
            status,
            payload,
            attempts: value.attempts,
            max_attempts: value.max_attempts,
            last_error: value.last_error,
            run_after: value.run_after,
            created_at: value.created_at,
            updated_at: value.updated_at,
        })
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, SurrealValue)]
pub(super) struct SessionRow {
    pub(super) uid: String,
    pub(super) game_id: String,
    pub(super) title: String,
    pub(super) created_at: DateTime<Utc>,
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
pub(super) struct MessageRow {
    pub(super) uid: String,
    pub(super) session_id: String,
    pub(super) role: String,
    pub(super) content: String,
    pub(super) created_at: DateTime<Utc>,
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
pub(super) struct SummaryRow {
    pub(super) uid: String,
    pub(super) session_id: String,
    pub(super) content: String,
    pub(super) updated_at: DateTime<Utc>,
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
pub(super) struct CharacterRow {
    pub(super) uid: String,
    pub(super) game_id: String,
    pub(super) name: String,
    pub(super) description: String,
    pub(super) status: String,
    pub(super) updated_at: DateTime<Utc>,
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
pub(super) struct EventRow {
    pub(super) uid: String,
    pub(super) session_id: String,
    pub(super) summary: String,
    pub(super) importance: i32,
    pub(super) created_at: DateTime<Utc>,
}

impl From<EventRow> for Event {
    fn from(value: EventRow) -> Self {
        Self {
            id: value.uid,
            session_id: value.session_id,
            summary: value.summary,
            importance: value.importance,
            created_at: value.created_at,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, SurrealValue)]
pub(super) struct LocationRow {
    pub(super) uid: String,
    pub(super) game_id: String,
    pub(super) name: String,
    pub(super) description: String,
    pub(super) updated_at: DateTime<Utc>,
}

impl From<LocationRow> for Location {
    fn from(value: LocationRow) -> Self {
        Self {
            id: value.uid,
            game_id: value.game_id,
            name: value.name,
            description: value.description,
            updated_at: value.updated_at,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, SurrealValue)]
pub(super) struct WorldFactRow {
    pub(super) uid: String,
    pub(super) game_id: String,
    pub(super) subject: String,
    pub(super) predicate: String,
    pub(super) object: String,
    pub(super) content: String,
    pub(super) confidence: f32,
    pub(super) updated_at: DateTime<Utc>,
}

impl From<WorldFactRow> for WorldFact {
    fn from(value: WorldFactRow) -> Self {
        Self {
            id: value.uid,
            game_id: value.game_id,
            subject: value.subject,
            predicate: value.predicate,
            object: value.object,
            content: value.content,
            confidence: value.confidence,
            updated_at: value.updated_at,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, SurrealValue)]
pub(super) struct GraphEdgeRow {
    pub(super) in_record: RecordId,
    pub(super) out_record: RecordId,
    pub(super) created_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Serialize, Deserialize, SurrealValue)]
pub(super) struct MemoryChunkRow {
    pub(super) uid: String,
    pub(super) session_id: String,
    pub(super) kind: String,
    pub(super) content: String,
    #[serde(default)]
    pub(super) embedding_16: Option<Vec<f32>>,
    #[serde(default)]
    pub(super) embedding_384: Option<Vec<f32>>,
    #[serde(default)]
    pub(super) embedding_768: Option<Vec<f32>>,
    #[serde(default)]
    pub(super) embedding_1024: Option<Vec<f32>>,
    #[serde(default)]
    pub(super) embedding_1536: Option<Vec<f32>>,
    #[serde(default)]
    pub(super) embedding_3072: Option<Vec<f32>>,
    pub(super) embedding: Vec<f32>,
    pub(super) created_at: DateTime<Utc>,
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

#[derive(Clone, Debug, Serialize, Deserialize, SurrealValue)]
pub(super) struct MemorySearchRow {
    pub(super) uid: String,
    pub(super) session_id: String,
    pub(super) kind: String,
    pub(super) content: String,
    #[serde(default)]
    pub(super) embedding_16: Option<Vec<f32>>,
    #[serde(default)]
    pub(super) embedding_384: Option<Vec<f32>>,
    #[serde(default)]
    pub(super) embedding_768: Option<Vec<f32>>,
    #[serde(default)]
    pub(super) embedding_1024: Option<Vec<f32>>,
    #[serde(default)]
    pub(super) embedding_1536: Option<Vec<f32>>,
    #[serde(default)]
    pub(super) embedding_3072: Option<Vec<f32>>,
    pub(super) embedding: Vec<f32>,
    #[serde(default)]
    pub(super) lexical_score: Option<f32>,
    pub(super) created_at: DateTime<Utc>,
}

#[derive(Clone, Debug)]
pub(super) struct MemorySearchCandidate {
    pub(super) row: MemoryChunkRow,
    pub(super) lexical_score: Option<f32>,
}

impl From<MemoryChunkRow> for MemorySearchCandidate {
    fn from(row: MemoryChunkRow) -> Self {
        Self {
            row,
            lexical_score: None,
        }
    }
}

impl From<MemorySearchRow> for MemorySearchCandidate {
    fn from(value: MemorySearchRow) -> Self {
        Self {
            row: MemoryChunkRow {
                uid: value.uid,
                session_id: value.session_id,
                kind: value.kind,
                content: value.content,
                embedding_16: value.embedding_16,
                embedding_384: value.embedding_384,
                embedding_768: value.embedding_768,
                embedding_1024: value.embedding_1024,
                embedding_1536: value.embedding_1536,
                embedding_3072: value.embedding_3072,
                embedding: value.embedding,
                created_at: value.created_at,
            },
            lexical_score: value.lexical_score,
        }
    }
}
