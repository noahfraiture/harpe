use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

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
