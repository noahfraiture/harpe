use chrono::{DateTime, Utc};

pub(super) const MIGRATION_BOOTSTRAP: &str = r#"
DEFINE TABLE OVERWRITE schema_migration SCHEMAFULL;
DEFINE FIELD OVERWRITE uid ON schema_migration TYPE string;
DEFINE FIELD OVERWRITE version ON schema_migration TYPE int;
DEFINE FIELD OVERWRITE name ON schema_migration TYPE string;
DEFINE FIELD OVERWRITE applied_at ON schema_migration TYPE datetime;
DEFINE INDEX OVERWRITE schema_migration_version ON schema_migration FIELDS version UNIQUE;
"#;

pub(super) const MIGRATIONS: &[Migration] = &[
    Migration {
        version: 1,
        name: "core_schema",
        sql: r#"
DEFINE TABLE OVERWRITE game SCHEMAFULL;
DEFINE FIELD OVERWRITE uid ON game TYPE string;
DEFINE FIELD OVERWRITE title ON game TYPE string;
DEFINE FIELD OVERWRITE system_prompt ON game TYPE string;
DEFINE FIELD OVERWRITE created_at ON game TYPE datetime;
DEFINE INDEX OVERWRITE game_created_at ON game FIELDS created_at;

DEFINE TABLE OVERWRITE session SCHEMAFULL;
DEFINE FIELD OVERWRITE uid ON session TYPE string;
DEFINE FIELD OVERWRITE game_id ON session TYPE string;
DEFINE FIELD OVERWRITE title ON session TYPE string;
DEFINE FIELD OVERWRITE created_at ON session TYPE datetime;
DEFINE INDEX OVERWRITE session_game_id ON session FIELDS game_id;
DEFINE INDEX OVERWRITE session_game_created_at ON session FIELDS game_id, created_at;

DEFINE TABLE OVERWRITE message SCHEMAFULL;
DEFINE FIELD OVERWRITE uid ON message TYPE string;
DEFINE FIELD OVERWRITE session_id ON message TYPE string;
DEFINE FIELD OVERWRITE role ON message TYPE string;
DEFINE FIELD OVERWRITE content ON message TYPE string;
DEFINE FIELD OVERWRITE created_at ON message TYPE datetime;
DEFINE INDEX OVERWRITE message_session_id ON message FIELDS session_id;
DEFINE INDEX OVERWRITE message_session_created_at ON message FIELDS session_id, created_at;

DEFINE TABLE OVERWRITE summary SCHEMAFULL;
DEFINE FIELD OVERWRITE uid ON summary TYPE string;
DEFINE FIELD OVERWRITE session_id ON summary TYPE string;
DEFINE FIELD OVERWRITE content ON summary TYPE string;
DEFINE FIELD OVERWRITE updated_at ON summary TYPE datetime;
DEFINE INDEX OVERWRITE summary_session_id ON summary FIELDS session_id UNIQUE;

DEFINE TABLE OVERWRITE character SCHEMAFULL;
DEFINE FIELD OVERWRITE uid ON character TYPE string;
DEFINE FIELD OVERWRITE game_id ON character TYPE string;
DEFINE FIELD OVERWRITE name ON character TYPE string;
DEFINE FIELD OVERWRITE description ON character TYPE string;
DEFINE FIELD OVERWRITE status ON character TYPE string;
DEFINE FIELD OVERWRITE updated_at ON character TYPE datetime;
DEFINE INDEX OVERWRITE character_game_id ON character FIELDS game_id;
DEFINE INDEX OVERWRITE character_game_name ON character FIELDS game_id, name;

DEFINE TABLE OVERWRITE event SCHEMAFULL;
DEFINE FIELD OVERWRITE uid ON event TYPE string;
DEFINE FIELD OVERWRITE session_id ON event TYPE string;
DEFINE FIELD OVERWRITE summary ON event TYPE string;
DEFINE FIELD OVERWRITE importance ON event TYPE int;
DEFINE FIELD OVERWRITE created_at ON event TYPE datetime;
DEFINE INDEX OVERWRITE event_session_id ON event FIELDS session_id;
DEFINE INDEX OVERWRITE event_session_created_at ON event FIELDS session_id, created_at;

DEFINE TABLE OVERWRITE location SCHEMAFULL;
DEFINE FIELD OVERWRITE uid ON location TYPE string;
DEFINE FIELD OVERWRITE game_id ON location TYPE string;
DEFINE FIELD OVERWRITE name ON location TYPE string;
DEFINE FIELD OVERWRITE description ON location TYPE string;
DEFINE FIELD OVERWRITE updated_at ON location TYPE datetime;
DEFINE INDEX OVERWRITE location_game_id ON location FIELDS game_id;
DEFINE INDEX OVERWRITE location_game_name ON location FIELDS game_id, name;

DEFINE TABLE OVERWRITE world_fact SCHEMAFULL;
DEFINE FIELD OVERWRITE uid ON world_fact TYPE string;
DEFINE FIELD OVERWRITE game_id ON world_fact TYPE string;
DEFINE FIELD OVERWRITE subject ON world_fact TYPE string;
DEFINE FIELD OVERWRITE predicate ON world_fact TYPE string;
DEFINE FIELD OVERWRITE object ON world_fact TYPE string;
DEFINE FIELD OVERWRITE content ON world_fact TYPE string;
DEFINE FIELD OVERWRITE confidence ON world_fact TYPE float;
DEFINE FIELD OVERWRITE updated_at ON world_fact TYPE datetime;
DEFINE INDEX OVERWRITE world_fact_game_id ON world_fact FIELDS game_id;
DEFINE INDEX OVERWRITE world_fact_triplet ON world_fact FIELDS game_id, subject, predicate, object;

DEFINE TABLE OVERWRITE memory_chunk SCHEMAFULL;
DEFINE FIELD OVERWRITE uid ON memory_chunk TYPE string;
DEFINE FIELD OVERWRITE session_id ON memory_chunk TYPE string;
DEFINE FIELD OVERWRITE kind ON memory_chunk TYPE string;
DEFINE FIELD OVERWRITE content ON memory_chunk TYPE string;
DEFINE FIELD OVERWRITE embedding ON memory_chunk TYPE array<float>;
DEFINE FIELD OVERWRITE created_at ON memory_chunk TYPE datetime;
DEFINE INDEX OVERWRITE memory_chunk_session_id ON memory_chunk FIELDS session_id;
DEFINE INDEX OVERWRITE memory_chunk_session_kind ON memory_chunk FIELDS session_id, kind;
"#,
    },
    Migration {
        version: 2,
        name: "graph_relations",
        sql: r#"
DEFINE TABLE OVERWRITE session_in_game TYPE RELATION IN session OUT game SCHEMAFULL;
DEFINE FIELD OVERWRITE created_at ON session_in_game TYPE datetime;
DEFINE INDEX OVERWRITE session_in_game_pair ON session_in_game FIELDS in, out UNIQUE;

DEFINE TABLE OVERWRITE message_in_session TYPE RELATION IN message OUT session SCHEMAFULL;
DEFINE FIELD OVERWRITE created_at ON message_in_session TYPE datetime;
DEFINE INDEX OVERWRITE message_in_session_pair ON message_in_session FIELDS in, out UNIQUE;

DEFINE TABLE OVERWRITE event_in_session TYPE RELATION IN event OUT session SCHEMAFULL;
DEFINE FIELD OVERWRITE created_at ON event_in_session TYPE datetime;
DEFINE INDEX OVERWRITE event_in_session_pair ON event_in_session FIELDS in, out UNIQUE;

DEFINE TABLE OVERWRITE character_in_game TYPE RELATION IN character OUT game SCHEMAFULL;
DEFINE FIELD OVERWRITE created_at ON character_in_game TYPE datetime;
DEFINE INDEX OVERWRITE character_in_game_pair ON character_in_game FIELDS in, out UNIQUE;

DEFINE TABLE OVERWRITE location_in_game TYPE RELATION IN location OUT game SCHEMAFULL;
DEFINE FIELD OVERWRITE created_at ON location_in_game TYPE datetime;
DEFINE INDEX OVERWRITE location_in_game_pair ON location_in_game FIELDS in, out UNIQUE;

DEFINE TABLE OVERWRITE world_fact_in_game TYPE RELATION IN world_fact OUT game SCHEMAFULL;
DEFINE FIELD OVERWRITE created_at ON world_fact_in_game TYPE datetime;
DEFINE INDEX OVERWRITE world_fact_in_game_pair ON world_fact_in_game FIELDS in, out UNIQUE;

DEFINE TABLE OVERWRITE memory_in_session TYPE RELATION IN memory_chunk OUT session SCHEMAFULL;
DEFINE FIELD OVERWRITE created_at ON memory_in_session TYPE datetime;
DEFINE INDEX OVERWRITE memory_in_session_pair ON memory_in_session FIELDS in, out UNIQUE;

DEFINE TABLE OVERWRITE event_involves_character TYPE RELATION IN event OUT character SCHEMAFULL;
DEFINE FIELD OVERWRITE created_at ON event_involves_character TYPE datetime;
DEFINE INDEX OVERWRITE event_involves_character_pair ON event_involves_character FIELDS in, out UNIQUE;

DEFINE TABLE OVERWRITE event_happened_at_location TYPE RELATION IN event OUT location SCHEMAFULL;
DEFINE FIELD OVERWRITE created_at ON event_happened_at_location TYPE datetime;
DEFINE INDEX OVERWRITE event_happened_at_location_pair ON event_happened_at_location FIELDS in, out UNIQUE;

DEFINE TABLE OVERWRITE character_knows_world_fact TYPE RELATION IN character OUT world_fact SCHEMAFULL;
DEFINE FIELD OVERWRITE created_at ON character_knows_world_fact TYPE datetime;
DEFINE INDEX OVERWRITE character_knows_world_fact_pair ON character_knows_world_fact FIELDS in, out UNIQUE;

DEFINE TABLE OVERWRITE memory_supports_world_fact TYPE RELATION IN memory_chunk OUT world_fact SCHEMAFULL;
DEFINE FIELD OVERWRITE created_at ON memory_supports_world_fact TYPE datetime;
DEFINE INDEX OVERWRITE memory_supports_world_fact_pair ON memory_supports_world_fact FIELDS in, out UNIQUE;
"#,
    },
    Migration {
        version: 3,
        name: "background_jobs",
        sql: r#"
DEFINE TABLE OVERWRITE background_job SCHEMAFULL;
DEFINE FIELD OVERWRITE uid ON background_job TYPE string;
DEFINE FIELD OVERWRITE kind ON background_job TYPE string;
DEFINE FIELD OVERWRITE status ON background_job TYPE string;
DEFINE FIELD OVERWRITE payload_json ON background_job TYPE string;
DEFINE FIELD OVERWRITE attempts ON background_job TYPE int;
DEFINE FIELD OVERWRITE max_attempts ON background_job TYPE int;
DEFINE FIELD OVERWRITE last_error ON background_job TYPE option<string>;
DEFINE FIELD OVERWRITE run_after ON background_job TYPE datetime;
DEFINE FIELD OVERWRITE created_at ON background_job TYPE datetime;
DEFINE FIELD OVERWRITE updated_at ON background_job TYPE datetime;
DEFINE INDEX OVERWRITE background_job_status_run_after ON background_job FIELDS status, run_after;
DEFINE INDEX OVERWRITE background_job_status_created_at ON background_job FIELDS status, created_at;
"#,
    },
    Migration {
        version: 4,
        name: "user_ownership",
        sql: r#"
DEFINE TABLE OVERWRITE user_account SCHEMAFULL;
DEFINE FIELD OVERWRITE uid ON user_account TYPE string;
DEFINE FIELD OVERWRITE display_name ON user_account TYPE string;
DEFINE FIELD OVERWRITE created_at ON user_account TYPE datetime;
DEFINE INDEX OVERWRITE user_account_created_at ON user_account FIELDS created_at;

DEFINE FIELD OVERWRITE owner_user_id ON game TYPE string DEFAULT "";
UPDATE game SET owner_user_id = "" WHERE owner_user_id = NONE;
DEFINE INDEX OVERWRITE game_owner_user_id ON game FIELDS owner_user_id;
DEFINE INDEX OVERWRITE game_owner_created_at ON game FIELDS owner_user_id, created_at;
"#,
    },
    Migration {
        version: 5,
        name: "indexed_memory_search",
        sql: r#"
DEFINE ANALYZER OVERWRITE memory_content_analyzer TOKENIZERS blank, class, camel, punct FILTERS lowercase, ascii;
DEFINE FIELD OVERWRITE embedding_16 ON memory_chunk TYPE option<array<float>>;
DEFINE INDEX OVERWRITE memory_chunk_content_fulltext ON TABLE memory_chunk FIELDS content FULLTEXT ANALYZER memory_content_analyzer BM25;
DEFINE INDEX OVERWRITE memory_chunk_embedding_16_hnsw ON TABLE memory_chunk FIELDS embedding_16 HNSW DIMENSION 16 DIST COSINE TYPE F32;
"#,
    },
    Migration {
        version: 6,
        name: "provider_embedding_indexes",
        sql: r#"
DEFINE FIELD OVERWRITE embedding_384 ON memory_chunk TYPE option<array<float>>;
DEFINE FIELD OVERWRITE embedding_768 ON memory_chunk TYPE option<array<float>>;
DEFINE FIELD OVERWRITE embedding_1024 ON memory_chunk TYPE option<array<float>>;
DEFINE FIELD OVERWRITE embedding_1536 ON memory_chunk TYPE option<array<float>>;
DEFINE FIELD OVERWRITE embedding_3072 ON memory_chunk TYPE option<array<float>>;
DEFINE INDEX OVERWRITE memory_chunk_embedding_384_hnsw ON TABLE memory_chunk FIELDS embedding_384 HNSW DIMENSION 384 DIST COSINE TYPE F32;
DEFINE INDEX OVERWRITE memory_chunk_embedding_768_hnsw ON TABLE memory_chunk FIELDS embedding_768 HNSW DIMENSION 768 DIST COSINE TYPE F32;
DEFINE INDEX OVERWRITE memory_chunk_embedding_1024_hnsw ON TABLE memory_chunk FIELDS embedding_1024 HNSW DIMENSION 1024 DIST COSINE TYPE F32;
DEFINE INDEX OVERWRITE memory_chunk_embedding_1536_hnsw ON TABLE memory_chunk FIELDS embedding_1536 HNSW DIMENSION 1536 DIST COSINE TYPE F32;
DEFINE INDEX OVERWRITE memory_chunk_embedding_3072_hnsw ON TABLE memory_chunk FIELDS embedding_3072 HNSW DIMENSION 3072 DIST COSINE TYPE F32;
"#,
    },
];

#[derive(Clone, Copy, Debug)]
pub(super) struct Migration {
    pub(super) version: i32,
    pub(super) name: &'static str,
    pub(super) sql: &'static str,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AppliedMigration {
    pub version: i32,
    pub name: String,
    pub applied_at: DateTime<Utc>,
}
