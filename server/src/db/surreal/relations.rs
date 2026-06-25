use crate::domain::GraphRelationKind;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct RelationSpec {
    pub(super) table: &'static str,
    pub(super) in_table: &'static str,
    pub(super) out_table: &'static str,
}

pub(super) fn relation_spec(relation: GraphRelationKind) -> RelationSpec {
    match relation {
        GraphRelationKind::SessionInGame => RelationSpec {
            table: "session_in_game",
            in_table: "session",
            out_table: "game",
        },
        GraphRelationKind::MessageInSession => RelationSpec {
            table: "message_in_session",
            in_table: "message",
            out_table: "session",
        },
        GraphRelationKind::EventInSession => RelationSpec {
            table: "event_in_session",
            in_table: "event",
            out_table: "session",
        },
        GraphRelationKind::CharacterInGame => RelationSpec {
            table: "character_in_game",
            in_table: "character",
            out_table: "game",
        },
        GraphRelationKind::LocationInGame => RelationSpec {
            table: "location_in_game",
            in_table: "location",
            out_table: "game",
        },
        GraphRelationKind::WorldFactInGame => RelationSpec {
            table: "world_fact_in_game",
            in_table: "world_fact",
            out_table: "game",
        },
        GraphRelationKind::MemoryInSession => RelationSpec {
            table: "memory_in_session",
            in_table: "memory_chunk",
            out_table: "session",
        },
        GraphRelationKind::EventInvolvesCharacter => RelationSpec {
            table: "event_involves_character",
            in_table: "event",
            out_table: "character",
        },
        GraphRelationKind::EventHappenedAtLocation => RelationSpec {
            table: "event_happened_at_location",
            in_table: "event",
            out_table: "location",
        },
        GraphRelationKind::CharacterKnowsWorldFact => RelationSpec {
            table: "character_knows_world_fact",
            in_table: "character",
            out_table: "world_fact",
        },
        GraphRelationKind::MemorySupportsWorldFact => RelationSpec {
            table: "memory_supports_world_fact",
            in_table: "memory_chunk",
            out_table: "world_fact",
        },
    }
}

pub(super) fn edge_id(in_record_id: &str, out_record_id: &str) -> String {
    format!(
        "{}__{}",
        sanitize_record_key(in_record_id),
        sanitize_record_key(out_record_id)
    )
}

fn sanitize_record_key(value: &str) -> String {
    value
        .chars()
        .map(|char| match char {
            'a'..='z' | 'A'..='Z' | '0'..='9' => char,
            _ => '_',
        })
        .collect()
}
