use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

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
