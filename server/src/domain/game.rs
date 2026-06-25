use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::{Character, Event, Location, MemoryChunk, Session, StorySummary, WorldFact};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Game {
    pub id: String,
    pub owner_user_id: String,
    pub title: String,
    pub system_prompt: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GameSnapshot {
    pub game: Game,
    pub sessions: Vec<Session>,
    pub summaries: Vec<StorySummary>,
    pub characters: Vec<Character>,
    pub events: Vec<Event>,
    pub world_facts: Vec<WorldFact>,
    pub locations: Vec<Location>,
    pub memory_chunks: Vec<MemoryChunk>,
    pub exported_at: DateTime<Utc>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NewGame {
    pub owner_user_id: String,
    pub title: String,
    pub system_prompt: String,
}
