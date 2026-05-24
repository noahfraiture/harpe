use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub fn new_id() -> String {
    Uuid::now_v7().to_string()
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Game {
    pub id: String,
    pub title: String,
    pub system_prompt: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NewGame {
    pub title: String,
    pub system_prompt: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    pub game_id: String,
    pub title: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NewSession {
    pub game_id: String,
    pub title: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum MessageRole {
    System,
    User,
    Assistant,
}

impl MessageRole {
    pub const fn as_db_value(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::User => "user",
            Self::Assistant => "assistant",
        }
    }

    pub fn from_db_value(value: &str) -> Option<Self> {
        match value {
            "system" => Some(Self::System),
            "user" => Some(Self::User),
            "assistant" => Some(Self::Assistant),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Message {
    pub id: String,
    pub session_id: String,
    pub role: MessageRole,
    pub content: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NewMessage {
    pub id: Option<String>,
    pub session_id: String,
    pub role: MessageRole,
    pub content: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StorySummary {
    pub session_id: String,
    pub content: String,
    pub updated_at: DateTime<Utc>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UpsertStorySummary {
    pub session_id: String,
    pub content: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Character {
    pub id: String,
    pub game_id: String,
    pub name: String,
    pub description: String,
    pub status: String,
    pub updated_at: DateTime<Utc>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UpsertCharacter {
    pub id: Option<String>,
    pub game_id: String,
    pub name: String,
    pub description: String,
    pub status: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Event {
    pub id: String,
    pub session_id: String,
    pub summary: String,
    pub importance: i32,
    pub created_at: DateTime<Utc>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NewEvent {
    pub session_id: String,
    pub summary: String,
    pub importance: i32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Location {
    pub id: String,
    pub game_id: String,
    pub name: String,
    pub description: String,
    pub updated_at: DateTime<Utc>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UpsertLocation {
    pub id: Option<String>,
    pub game_id: String,
    pub name: String,
    pub description: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WorldFact {
    pub id: String,
    pub game_id: String,
    pub subject: String,
    pub predicate: String,
    pub object: String,
    pub content: String,
    pub confidence: f32,
    pub updated_at: DateTime<Utc>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct UpsertWorldFact {
    pub id: Option<String>,
    pub game_id: String,
    pub subject: String,
    pub predicate: String,
    pub object: String,
    pub content: String,
    pub confidence: f32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MemoryChunk {
    pub id: String,
    pub session_id: String,
    pub kind: String,
    pub content: String,
    pub embedding: Vec<f32>,
    pub created_at: DateTime<Utc>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct NewMemoryChunk {
    pub session_id: String,
    pub kind: String,
    pub content: String,
    pub embedding: Vec<f32>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MemoryHit {
    pub chunk: MemoryChunk,
    pub score: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum GraphRelationKind {
    SessionInGame,
    MessageInSession,
    EventInSession,
    CharacterInGame,
    LocationInGame,
    WorldFactInGame,
    MemoryInSession,
    EventInvolvesCharacter,
    EventHappenedAtLocation,
    CharacterKnowsWorldFact,
    MemorySupportsWorldFact,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GraphEdge {
    pub relation: GraphRelationKind,
    pub in_record: String,
    pub out_record: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct MemoryExtraction {
    pub events: Vec<ExtractedEvent>,
    pub character_updates: Vec<ExtractedCharacterUpdate>,
    pub world_facts: Vec<ExtractedWorldFact>,
    pub locations: Vec<ExtractedLocation>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExtractedEvent {
    pub summary: String,
    pub importance: i32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExtractedCharacterUpdate {
    pub name: String,
    pub description: String,
    pub status: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ExtractedWorldFact {
    pub subject: String,
    pub predicate: String,
    pub object: String,
    pub content: String,
    pub confidence: f32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExtractedLocation {
    pub name: String,
    pub description: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn message_role_round_trips_db_value() {
        for role in [
            MessageRole::System,
            MessageRole::User,
            MessageRole::Assistant,
        ] {
            assert_eq!(MessageRole::from_db_value(role.as_db_value()), Some(role));
        }
    }

    #[test]
    fn unknown_message_role_is_rejected() {
        assert_eq!(MessageRole::from_db_value("narrator"), None);
    }

    #[test]
    fn new_ids_are_non_empty_and_distinct() {
        let first = new_id();
        let second = new_id();

        assert!(!first.is_empty());
        assert!(!second.is_empty());
        assert_ne!(first, second);
    }

    #[test]
    fn empty_memory_extraction_has_no_updates() {
        let extraction = MemoryExtraction::default();

        assert!(extraction.events.is_empty());
        assert!(extraction.character_updates.is_empty());
        assert!(extraction.world_facts.is_empty());
        assert!(extraction.locations.is_empty());
    }

    #[test]
    fn graph_relation_kind_serializes_for_storage_boundaries() {
        let relation = GraphRelationKind::EventInvolvesCharacter;
        let encoded = serde_json::to_string(&relation).unwrap();
        let decoded: GraphRelationKind = serde_json::from_str(&encoded).unwrap();

        assert_eq!(decoded, relation);
    }
}
