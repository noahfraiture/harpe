use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

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
