use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

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

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct MemoryExtraction {
    #[serde(default)]
    pub events: Vec<ExtractedEvent>,
    #[serde(default)]
    pub character_updates: Vec<ExtractedCharacterUpdate>,
    #[serde(default)]
    pub world_facts: Vec<ExtractedWorldFact>,
    #[serde(default)]
    pub locations: Vec<ExtractedLocation>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExtractedEvent {
    pub summary: String,
    pub importance: i32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExtractedCharacterUpdate {
    pub name: String,
    pub description: String,
    pub status: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ExtractedWorldFact {
    pub subject: String,
    pub predicate: String,
    pub object: String,
    pub content: String,
    pub confidence: f32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExtractedLocation {
    pub name: String,
    pub description: String,
}
