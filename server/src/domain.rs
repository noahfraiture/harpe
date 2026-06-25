mod character;
mod event;
mod game;
mod graph;
mod ids;
mod job;
mod location;
mod memory;
mod message;
mod session;
mod user;
mod world_fact;

pub use character::{Character, UpsertCharacter};
pub use event::{Event, NewEvent};
pub use game::{Game, GameSnapshot, NewGame};
pub use graph::{GraphEdge, GraphRelationKind};
pub use ids::new_id;
pub use job::{BackgroundJob, JobKind, JobStatus, NewBackgroundJob};
pub use location::{Location, UpsertLocation};
pub use memory::{
    ExtractedCharacterUpdate, ExtractedEvent, ExtractedLocation, ExtractedWorldFact, MemoryChunk,
    MemoryExtraction, MemoryHit, NewMemoryChunk, StorySummary, UpsertStorySummary,
};
pub use message::{Message, MessageRole, NewMessage};
pub use session::{NewSession, Session};
pub use user::{NewUser, User};
pub use world_fact::{UpsertWorldFact, WorldFact};

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

    #[test]
    fn job_kind_round_trips_db_value() {
        let kind = JobKind::UpdateMemoryAfterTurn;

        assert_eq!(JobKind::from_db_value(kind.as_db_value()), Some(kind));
        assert_eq!(JobKind::from_db_value("unknown"), None);
    }

    #[test]
    fn job_status_round_trips_db_value() {
        for status in [
            JobStatus::Pending,
            JobStatus::Running,
            JobStatus::Succeeded,
            JobStatus::Failed,
        ] {
            assert_eq!(JobStatus::from_db_value(status.as_db_value()), Some(status));
        }

        assert_eq!(JobStatus::from_db_value("paused"), None);
    }
}
