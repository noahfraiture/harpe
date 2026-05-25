use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use surrealdb::Surreal;
use surrealdb::engine::any::{self, Any};
use surrealdb::types::{RecordId, SurrealValue, ToSql};

use crate::domain::{
    BackgroundJob, Character, Event, Game, GraphEdge, GraphRelationKind, JobKind, JobStatus,
    Location, MemoryChunk, MemoryHit, Message, MessageRole, NewBackgroundJob, NewEvent, NewGame,
    NewMemoryChunk, NewMessage, NewSession, NewUser, Session, StorySummary, UpsertCharacter,
    UpsertLocation, UpsertStorySummary, UpsertWorldFact, User, WorldFact, new_id,
};
use crate::engine::cosine_similarity;
use crate::store::HarpeStore;
use crate::{HarpeError, Result};

const MIGRATION_BOOTSTRAP: &str = r#"
DEFINE TABLE OVERWRITE schema_migration SCHEMAFULL;
DEFINE FIELD OVERWRITE uid ON schema_migration TYPE string;
DEFINE FIELD OVERWRITE version ON schema_migration TYPE int;
DEFINE FIELD OVERWRITE name ON schema_migration TYPE string;
DEFINE FIELD OVERWRITE applied_at ON schema_migration TYPE datetime;
DEFINE INDEX OVERWRITE schema_migration_version ON schema_migration FIELDS version UNIQUE;
"#;

const MIGRATIONS: &[Migration] = &[
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
];

#[derive(Clone, Copy, Debug)]
struct Migration {
    version: i32,
    name: &'static str,
    sql: &'static str,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AppliedMigration {
    pub version: i32,
    pub name: String,
    pub applied_at: DateTime<Utc>,
}

#[derive(Clone)]
pub struct SurrealStore {
    db: Arc<Surreal<Any>>,
}

impl SurrealStore {
    pub async fn connect(
        endpoint: impl Into<String>,
        namespace: &str,
        database: &str,
    ) -> Result<Self> {
        let db = any::connect(endpoint.into()).await?;
        db.use_ns(namespace).use_db(database).await?;

        let store = Self { db: Arc::new(db) };
        store.migrate().await?;

        Ok(store)
    }

    pub async fn migrate(&self) -> Result<()> {
        self.db.query(MIGRATION_BOOTSTRAP).await?.check()?;
        let applied_versions = self
            .applied_migrations()
            .await?
            .into_iter()
            .map(|migration| migration.version)
            .collect::<Vec<_>>();

        for migration in MIGRATIONS {
            if applied_versions.contains(&migration.version) {
                continue;
            }

            self.db.query(migration.sql).await?.check()?;
            self.record_migration(*migration).await?;
        }

        Ok(())
    }

    pub async fn applied_migrations(&self) -> Result<Vec<AppliedMigration>> {
        let mut response = self
            .db
            .query("SELECT * FROM schema_migration ORDER BY version ASC")
            .await?;
        let rows: Vec<MigrationRow> = response.take(0)?;

        Ok(rows.into_iter().map(Into::into).collect())
    }

    pub fn migration_versions() -> Vec<i32> {
        MIGRATIONS
            .iter()
            .map(|migration| migration.version)
            .collect()
    }

    pub fn db(&self) -> &Surreal<Any> {
        &self.db
    }

    async fn record_migration(&self, migration: Migration) -> Result<()> {
        let row = MigrationRow {
            uid: migration_id(migration),
            version: migration.version,
            name: migration.name.to_owned(),
            applied_at: Utc::now(),
        };

        let _: Option<MigrationRow> = self
            .db
            .upsert(("schema_migration", row.uid.as_str()))
            .content(row)
            .await?;

        Ok(())
    }

    async fn upsert_graph_relation(
        &self,
        relation: GraphRelationKind,
        in_record_id: &str,
        out_record_id: &str,
    ) -> Result<()> {
        let spec = relation_spec(relation);
        let edge_id = edge_id(in_record_id, out_record_id);

        self.db
            .query(
                "LET $source_record = type::record($in_table, $in_id);
                 LET $target_record = type::record($out_table, $out_id);
                 LET $edge_record = type::record($relation_table, $edge_id);
                 DELETE $edge_record;
                 RELATE $source_record -> $edge_record -> $target_record
                 SET created_at = time::now();",
            )
            .bind(("relation_table", spec.table.to_owned()))
            .bind(("edge_id", edge_id))
            .bind(("in_table", spec.in_table.to_owned()))
            .bind(("in_id", in_record_id.to_owned()))
            .bind(("out_table", spec.out_table.to_owned()))
            .bind(("out_id", out_record_id.to_owned()))
            .await?
            .check()?;

        Ok(())
    }

    async fn update_job_state(
        &self,
        job_id: &str,
        status: JobStatus,
        attempts: Option<i32>,
        last_error: Option<String>,
    ) -> Result<BackgroundJob> {
        let mut row: JobRow = self
            .db
            .select(("background_job", job_id))
            .await?
            .ok_or_else(|| HarpeError::NotFound(format!("background job {job_id}")))?;

        row.status = status.as_db_value().to_owned();
        if let Some(attempts) = attempts {
            row.attempts = attempts;
        }
        row.last_error = last_error;
        row.updated_at = Utc::now();

        let updated: Option<JobRow> = self
            .db
            .update(("background_job", row.uid.as_str()))
            .content(row)
            .await?;

        updated
            .map(TryInto::try_into)
            .transpose()?
            .ok_or_else(|| HarpeError::Store("SurrealDB did not return updated job".to_owned()))
    }
}

#[async_trait]
impl HarpeStore for SurrealStore {
    async fn create_user(&self, input: NewUser) -> Result<User> {
        validate_present("display name", &input.display_name)?;

        let row = UserRow {
            uid: new_id(),
            display_name: input.display_name,
            created_at: Utc::now(),
        };

        let created: Option<UserRow> = self
            .db
            .create(("user_account", row.uid.as_str()))
            .content(row)
            .await?;

        created
            .map(Into::into)
            .ok_or_else(|| HarpeError::Store("SurrealDB did not return created user".to_owned()))
    }

    async fn get_user(&self, user_id: &str) -> Result<User> {
        let row: Option<UserRow> = self.db.select(("user_account", user_id)).await?;
        row.map(Into::into)
            .ok_or_else(|| HarpeError::NotFound(format!("user {user_id}")))
    }

    async fn create_game(&self, input: NewGame) -> Result<Game> {
        validate_present("owner user id", &input.owner_user_id)?;
        validate_present("game title", &input.title)?;
        self.get_user(&input.owner_user_id).await?;

        let row = GameRow {
            uid: new_id(),
            owner_user_id: input.owner_user_id,
            title: input.title,
            system_prompt: input.system_prompt,
            created_at: Utc::now(),
        };

        let created: Option<GameRow> = self
            .db
            .create(("game", row.uid.as_str()))
            .content(row)
            .await?;

        created
            .map(Into::into)
            .ok_or_else(|| HarpeError::Store("SurrealDB did not return created game".to_owned()))
    }

    async fn list_games(&self, limit: usize) -> Result<Vec<Game>> {
        let mut response = self
            .db
            .query("SELECT * FROM game ORDER BY created_at DESC LIMIT $limit")
            .bind(("limit", normalize_limit(limit) as i64))
            .await?;
        let rows: Vec<GameRow> = response.take(0)?;

        Ok(rows.into_iter().map(Into::into).collect())
    }

    async fn list_games_for_user(&self, owner_user_id: &str, limit: usize) -> Result<Vec<Game>> {
        let mut response = self
            .db
            .query(
                "SELECT * FROM game
                 WHERE owner_user_id = $owner_user_id
                 ORDER BY created_at DESC
                 LIMIT $limit",
            )
            .bind(("owner_user_id", owner_user_id.to_owned()))
            .bind(("limit", normalize_limit(limit) as i64))
            .await?;
        let rows: Vec<GameRow> = response.take(0)?;

        Ok(rows.into_iter().map(Into::into).collect())
    }

    async fn get_game(&self, game_id: &str) -> Result<Game> {
        let row: Option<GameRow> = self.db.select(("game", game_id)).await?;
        row.map(Into::into)
            .ok_or_else(|| HarpeError::NotFound(format!("game {game_id}")))
    }

    async fn enqueue_job(&self, input: NewBackgroundJob) -> Result<BackgroundJob> {
        let now = Utc::now();
        let row = JobRow {
            uid: new_id(),
            kind: input.kind.as_db_value().to_owned(),
            status: JobStatus::Pending.as_db_value().to_owned(),
            payload_json: serde_json::to_string(&input.payload)
                .map_err(|error| HarpeError::Store(error.to_string()))?,
            attempts: 0,
            max_attempts: normalize_max_attempts(input.max_attempts),
            last_error: None,
            run_after: input.run_after.unwrap_or(now),
            created_at: now,
            updated_at: now,
        };

        let created: Option<JobRow> = self
            .db
            .create(("background_job", row.uid.as_str()))
            .content(row)
            .await?;

        created
            .map(TryInto::try_into)
            .transpose()?
            .ok_or_else(|| HarpeError::Store("SurrealDB did not return created job".to_owned()))
    }

    async fn list_jobs(
        &self,
        status: Option<JobStatus>,
        limit: usize,
    ) -> Result<Vec<BackgroundJob>> {
        let rows: Vec<JobRow> = if let Some(status) = status {
            let mut response = self
                .db
                .query(
                    "SELECT * FROM background_job
                     WHERE status = $status
                     ORDER BY created_at DESC
                     LIMIT $limit",
                )
                .bind(("status", status.as_db_value().to_owned()))
                .bind(("limit", normalize_limit(limit) as i64))
                .await?;
            response.take(0)?
        } else {
            let mut response = self
                .db
                .query("SELECT * FROM background_job ORDER BY created_at DESC LIMIT $limit")
                .bind(("limit", normalize_limit(limit) as i64))
                .await?;
            response.take(0)?
        };

        rows.into_iter().map(TryInto::try_into).collect()
    }

    async fn claim_next_job(&self) -> Result<Option<BackgroundJob>> {
        let mut response = self
            .db
            .query(
                "SELECT * FROM background_job
                 WHERE status = $status AND run_after <= $now
                 ORDER BY run_after ASC, created_at ASC
                 LIMIT 1",
            )
            .bind(("status", JobStatus::Pending.as_db_value().to_owned()))
            .bind(("now", Utc::now()))
            .await?;
        let rows: Vec<JobRow> = response.take(0)?;
        let Some(row) = rows.into_iter().next() else {
            return Ok(None);
        };

        self.update_job_state(
            &row.uid,
            JobStatus::Running,
            Some(row.attempts.saturating_add(1)),
            None,
        )
        .await
        .map(Some)
    }

    async fn complete_job(&self, job_id: &str) -> Result<BackgroundJob> {
        self.update_job_state(job_id, JobStatus::Succeeded, None, None)
            .await
    }

    async fn fail_job(&self, job_id: &str, error: String) -> Result<BackgroundJob> {
        self.update_job_state(job_id, JobStatus::Failed, None, Some(error))
            .await
    }

    async fn create_session(&self, input: NewSession) -> Result<Session> {
        validate_present("game id", &input.game_id)?;
        validate_present("session title", &input.title)?;
        self.get_game(&input.game_id).await?;

        let row = SessionRow {
            uid: new_id(),
            game_id: input.game_id,
            title: input.title,
            created_at: Utc::now(),
        };

        let created: Option<SessionRow> = self
            .db
            .create(("session", row.uid.as_str()))
            .content(row)
            .await?;

        let session: Session = created.map(Into::into).ok_or_else(|| {
            HarpeError::Store("SurrealDB did not return created session".to_owned())
        })?;
        self.upsert_graph_relation(
            GraphRelationKind::SessionInGame,
            &session.id,
            &session.game_id,
        )
        .await?;

        Ok(session)
    }

    async fn list_sessions(&self, game_id: &str, limit: usize) -> Result<Vec<Session>> {
        let mut response = self
            .db
            .query(
                "SELECT * FROM session
                 WHERE game_id = $game_id
                 ORDER BY created_at ASC
                 LIMIT $limit",
            )
            .bind(("game_id", game_id.to_owned()))
            .bind(("limit", normalize_limit(limit) as i64))
            .await?;
        let rows: Vec<SessionRow> = response.take(0)?;

        Ok(rows.into_iter().map(Into::into).collect())
    }

    async fn get_session(&self, session_id: &str) -> Result<Session> {
        let row: Option<SessionRow> = self.db.select(("session", session_id)).await?;
        row.map(Into::into)
            .ok_or_else(|| HarpeError::NotFound(format!("session {session_id}")))
    }

    async fn append_message(&self, input: NewMessage) -> Result<Message> {
        validate_present("session id", &input.session_id)?;
        validate_present("message content", &input.content)?;

        let row = MessageRow {
            uid: input.id.unwrap_or_else(new_id),
            session_id: input.session_id,
            role: input.role.as_db_value().to_owned(),
            content: input.content,
            created_at: Utc::now(),
        };

        let created: Option<MessageRow> = self
            .db
            .create(("message", row.uid.as_str()))
            .content(row)
            .await?;

        let message: Message = created.map(TryInto::try_into).transpose()?.ok_or_else(|| {
            HarpeError::Store("SurrealDB did not return created message".to_owned())
        })?;
        self.upsert_graph_relation(
            GraphRelationKind::MessageInSession,
            &message.id,
            &message.session_id,
        )
        .await?;

        Ok(message)
    }

    async fn list_recent_messages(&self, session_id: &str, limit: usize) -> Result<Vec<Message>> {
        let mut response = self
            .db
            .query(
                "SELECT * FROM message WHERE session_id = $session_id ORDER BY created_at DESC LIMIT $limit",
            )
            .bind(("session_id", session_id.to_owned()))
            .bind(("limit", normalize_limit(limit) as i64))
            .await?;
        let mut rows: Vec<MessageRow> = response.take(0)?;
        rows.reverse();

        rows.into_iter().map(TryInto::try_into).collect()
    }

    async fn get_story_summary(&self, session_id: &str) -> Result<Option<StorySummary>> {
        let row: Option<SummaryRow> = self.db.select(("summary", session_id)).await?;
        Ok(row.map(Into::into))
    }

    async fn upsert_story_summary(&self, input: UpsertStorySummary) -> Result<StorySummary> {
        validate_present("session id", &input.session_id)?;

        let row = SummaryRow {
            uid: input.session_id.clone(),
            session_id: input.session_id,
            content: input.content,
            updated_at: Utc::now(),
        };

        let upserted: Option<SummaryRow> = self
            .db
            .upsert(("summary", row.uid.as_str()))
            .content(row)
            .await?;

        upserted.map(Into::into).ok_or_else(|| {
            HarpeError::Store("SurrealDB did not return upserted summary".to_owned())
        })
    }

    async fn upsert_character(&self, input: UpsertCharacter) -> Result<Character> {
        validate_present("game id", &input.game_id)?;
        validate_present("character name", &input.name)?;

        let uid = input.id.unwrap_or_else(new_id);
        let row = CharacterRow {
            uid,
            game_id: input.game_id,
            name: input.name,
            description: input.description,
            status: input.status,
            updated_at: Utc::now(),
        };

        let upserted: Option<CharacterRow> = self
            .db
            .upsert(("character", row.uid.as_str()))
            .content(row)
            .await?;

        let character: Character = upserted.map(Into::into).ok_or_else(|| {
            HarpeError::Store("SurrealDB did not return upserted character".to_owned())
        })?;
        self.upsert_graph_relation(
            GraphRelationKind::CharacterInGame,
            &character.id,
            &character.game_id,
        )
        .await?;

        Ok(character)
    }

    async fn list_characters(&self, game_id: &str) -> Result<Vec<Character>> {
        let mut response = self
            .db
            .query("SELECT * FROM character WHERE game_id = $game_id ORDER BY name ASC")
            .bind(("game_id", game_id.to_owned()))
            .await?;
        let rows: Vec<CharacterRow> = response.take(0)?;

        Ok(rows.into_iter().map(Into::into).collect())
    }

    async fn get_character(&self, character_id: &str) -> Result<Character> {
        let row: Option<CharacterRow> = self.db.select(("character", character_id)).await?;
        row.map(Into::into)
            .ok_or_else(|| HarpeError::NotFound(format!("character {character_id}")))
    }

    async fn save_event(&self, input: NewEvent) -> Result<Event> {
        validate_present("session id", &input.session_id)?;
        validate_present("event summary", &input.summary)?;

        let row = EventRow {
            uid: new_id(),
            session_id: input.session_id,
            summary: input.summary,
            importance: normalize_importance(input.importance),
            created_at: Utc::now(),
        };

        let created: Option<EventRow> = self
            .db
            .create(("event", row.uid.as_str()))
            .content(row)
            .await?;

        let event: Event = created.map(Into::into).ok_or_else(|| {
            HarpeError::Store("SurrealDB did not return created event".to_owned())
        })?;
        self.upsert_graph_relation(
            GraphRelationKind::EventInSession,
            &event.id,
            &event.session_id,
        )
        .await?;

        Ok(event)
    }

    async fn list_events(&self, session_id: &str, limit: usize) -> Result<Vec<Event>> {
        let mut response = self
            .db
            .query(
                "SELECT * FROM event WHERE session_id = $session_id ORDER BY created_at DESC LIMIT $limit",
            )
            .bind(("session_id", session_id.to_owned()))
            .bind(("limit", normalize_limit(limit) as i64))
            .await?;
        let mut rows: Vec<EventRow> = response.take(0)?;
        rows.reverse();

        Ok(rows.into_iter().map(Into::into).collect())
    }

    async fn upsert_location(&self, input: UpsertLocation) -> Result<Location> {
        validate_present("game id", &input.game_id)?;
        validate_present("location name", &input.name)?;

        let row = LocationRow {
            uid: input.id.unwrap_or_else(new_id),
            game_id: input.game_id,
            name: input.name,
            description: input.description,
            updated_at: Utc::now(),
        };

        let upserted: Option<LocationRow> = self
            .db
            .upsert(("location", row.uid.as_str()))
            .content(row)
            .await?;

        let location: Location = upserted.map(Into::into).ok_or_else(|| {
            HarpeError::Store("SurrealDB did not return upserted location".to_owned())
        })?;
        self.upsert_graph_relation(
            GraphRelationKind::LocationInGame,
            &location.id,
            &location.game_id,
        )
        .await?;

        Ok(location)
    }

    async fn list_locations(&self, game_id: &str) -> Result<Vec<Location>> {
        let mut response = self
            .db
            .query("SELECT * FROM location WHERE game_id = $game_id ORDER BY name ASC")
            .bind(("game_id", game_id.to_owned()))
            .await?;
        let rows: Vec<LocationRow> = response.take(0)?;

        Ok(rows.into_iter().map(Into::into).collect())
    }

    async fn upsert_world_fact(&self, input: UpsertWorldFact) -> Result<WorldFact> {
        validate_present("game id", &input.game_id)?;
        validate_present("world fact subject", &input.subject)?;
        validate_present("world fact predicate", &input.predicate)?;
        validate_present("world fact object", &input.object)?;

        let content = world_fact_content(
            &input.subject,
            &input.predicate,
            &input.object,
            &input.content,
        );
        let row = WorldFactRow {
            uid: input.id.unwrap_or_else(new_id),
            game_id: input.game_id,
            subject: input.subject,
            predicate: input.predicate,
            object: input.object,
            content,
            confidence: normalize_confidence(input.confidence),
            updated_at: Utc::now(),
        };

        let upserted: Option<WorldFactRow> = self
            .db
            .upsert(("world_fact", row.uid.as_str()))
            .content(row)
            .await?;

        let fact: WorldFact = upserted.map(Into::into).ok_or_else(|| {
            HarpeError::Store("SurrealDB did not return upserted world fact".to_owned())
        })?;
        self.upsert_graph_relation(GraphRelationKind::WorldFactInGame, &fact.id, &fact.game_id)
            .await?;

        Ok(fact)
    }

    async fn list_world_facts(&self, game_id: &str, limit: usize) -> Result<Vec<WorldFact>> {
        let mut response = self
            .db
            .query(
                "SELECT * FROM world_fact WHERE game_id = $game_id ORDER BY updated_at DESC LIMIT $limit",
            )
            .bind(("game_id", game_id.to_owned()))
            .bind(("limit", normalize_limit(limit) as i64))
            .await?;
        let rows: Vec<WorldFactRow> = response.take(0)?;

        Ok(rows.into_iter().map(Into::into).collect())
    }

    async fn list_graph_edges(
        &self,
        relation: GraphRelationKind,
        in_record_id: &str,
    ) -> Result<Vec<GraphEdge>> {
        let spec = relation_spec(relation);
        let mut response = self
            .db
            .query(
                "SELECT in AS in_record, out AS out_record, created_at
                 FROM type::table($relation_table)
                 WHERE in = type::record($in_table, $in_id)
                 ORDER BY created_at ASC",
            )
            .bind(("relation_table", spec.table.to_owned()))
            .bind(("in_table", spec.in_table.to_owned()))
            .bind(("in_id", in_record_id.to_owned()))
            .await?;
        let rows: Vec<GraphEdgeRow> = response.take(0)?;

        Ok(rows
            .into_iter()
            .map(|row| GraphEdge {
                relation,
                in_record: row.in_record.to_sql(),
                out_record: row.out_record.to_sql(),
                created_at: row.created_at,
            })
            .collect())
    }

    async fn save_memory_chunk(&self, input: NewMemoryChunk) -> Result<MemoryHit> {
        validate_present("session id", &input.session_id)?;
        validate_present("memory content", &input.content)?;

        let row = MemoryChunkRow {
            uid: new_id(),
            session_id: input.session_id,
            kind: input.kind,
            content: input.content,
            embedding: input.embedding,
            created_at: Utc::now(),
        };

        let created: Option<MemoryChunkRow> = self
            .db
            .create(("memory_chunk", row.uid.as_str()))
            .content(row)
            .await?;

        let hit = created
            .map(|row| MemoryHit {
                chunk: row.into(),
                score: 1.0,
            })
            .ok_or_else(|| {
                HarpeError::Store("SurrealDB did not return created memory chunk".to_owned())
            })?;
        self.upsert_graph_relation(
            GraphRelationKind::MemoryInSession,
            &hit.chunk.id,
            &hit.chunk.session_id,
        )
        .await?;

        Ok(hit)
    }

    async fn list_memory_chunks(&self, session_id: &str, limit: usize) -> Result<Vec<MemoryChunk>> {
        let mut response = self
            .db
            .query(
                "SELECT * FROM memory_chunk
                 WHERE session_id = $session_id
                 ORDER BY created_at ASC
                 LIMIT $limit",
            )
            .bind(("session_id", session_id.to_owned()))
            .bind(("limit", normalize_limit(limit) as i64))
            .await?;
        let rows: Vec<MemoryChunkRow> = response.take(0)?;

        Ok(rows.into_iter().map(Into::into).collect())
    }

    async fn search_memory(
        &self,
        session_id: &str,
        query: &str,
        query_embedding: &[f32],
        limit: usize,
    ) -> Result<Vec<MemoryHit>> {
        let mut response = self
            .db
            .query(
                "SELECT * FROM memory_chunk WHERE session_id = $session_id ORDER BY created_at DESC LIMIT 200",
            )
            .bind(("session_id", session_id.to_owned()))
            .await?;
        let rows: Vec<MemoryChunkRow> = response.take(0)?;
        let query = query.to_lowercase();

        let mut hits = rows
            .into_iter()
            .map(|row| {
                let vector_score = cosine_similarity(query_embedding, &row.embedding);
                let lexical_score = lexical_score(&query, &row.content);
                MemoryHit {
                    chunk: row.into(),
                    score: vector_score.max(lexical_score),
                }
            })
            .filter(|hit| hit.score > 0.0 || query.is_empty())
            .collect::<Vec<_>>();

        hits.sort_by(|left, right| {
            right
                .score
                .total_cmp(&left.score)
                .then_with(|| right.chunk.created_at.cmp(&left.chunk.created_at))
        });
        hits.truncate(normalize_limit(limit));

        Ok(hits)
    }
}

fn validate_present(label: &str, value: &str) -> Result<()> {
    if value.trim().is_empty() {
        return Err(HarpeError::Validation(format!("{label} is required")));
    }

    Ok(())
}

fn migration_id(migration: Migration) -> String {
    format!("m{:04}_{}", migration.version, migration.name)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct RelationSpec {
    table: &'static str,
    in_table: &'static str,
    out_table: &'static str,
}

fn relation_spec(relation: GraphRelationKind) -> RelationSpec {
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

fn edge_id(in_record_id: &str, out_record_id: &str) -> String {
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

fn normalize_limit(limit: usize) -> usize {
    match limit {
        0 => 50,
        1..=1_000 => limit,
        _ => 1_000,
    }
}

fn normalize_importance(importance: i32) -> i32 {
    importance.clamp(1, 5)
}

fn normalize_max_attempts(max_attempts: i32) -> i32 {
    max_attempts.clamp(1, 10)
}

fn normalize_confidence(confidence: f32) -> f32 {
    confidence.clamp(0.0, 1.0)
}

fn world_fact_content(subject: &str, predicate: &str, object: &str, content: &str) -> String {
    if content.trim().is_empty() {
        format!("{} {} {}", subject.trim(), predicate.trim(), object.trim())
    } else {
        content.to_owned()
    }
}

fn lexical_score(query: &str, content: &str) -> f32 {
    if query.is_empty() {
        return 0.0;
    }

    let content = content.to_lowercase();
    let terms = query
        .split_whitespace()
        .filter(|term| !term.is_empty())
        .collect::<Vec<_>>();

    if terms.is_empty() {
        return 0.0;
    }

    let matches = terms.iter().filter(|term| content.contains(**term)).count();

    matches as f32 / terms.len() as f32
}

#[derive(Clone, Debug, Serialize, Deserialize, SurrealValue)]
struct MigrationRow {
    uid: String,
    version: i32,
    name: String,
    applied_at: DateTime<Utc>,
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
struct UserRow {
    uid: String,
    display_name: String,
    created_at: DateTime<Utc>,
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
struct GameRow {
    uid: String,
    owner_user_id: String,
    title: String,
    system_prompt: String,
    created_at: DateTime<Utc>,
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
struct JobRow {
    uid: String,
    kind: String,
    status: String,
    payload_json: String,
    attempts: i32,
    max_attempts: i32,
    last_error: Option<String>,
    run_after: DateTime<Utc>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
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
struct SessionRow {
    uid: String,
    game_id: String,
    title: String,
    created_at: DateTime<Utc>,
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
struct MessageRow {
    uid: String,
    session_id: String,
    role: String,
    content: String,
    created_at: DateTime<Utc>,
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
struct SummaryRow {
    uid: String,
    session_id: String,
    content: String,
    updated_at: DateTime<Utc>,
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
struct CharacterRow {
    uid: String,
    game_id: String,
    name: String,
    description: String,
    status: String,
    updated_at: DateTime<Utc>,
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
struct EventRow {
    uid: String,
    session_id: String,
    summary: String,
    importance: i32,
    created_at: DateTime<Utc>,
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
struct LocationRow {
    uid: String,
    game_id: String,
    name: String,
    description: String,
    updated_at: DateTime<Utc>,
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
struct WorldFactRow {
    uid: String,
    game_id: String,
    subject: String,
    predicate: String,
    object: String,
    content: String,
    confidence: f32,
    updated_at: DateTime<Utc>,
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
struct GraphEdgeRow {
    in_record: RecordId,
    out_record: RecordId,
    created_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Serialize, Deserialize, SurrealValue)]
struct MemoryChunkRow {
    uid: String,
    session_id: String,
    kind: String,
    content: String,
    embedding: Vec<f32>,
    created_at: DateTime<Utc>,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lexical_score_counts_query_terms() {
        assert_eq!(
            lexical_score("silver key", "The silver door is locked."),
            0.5
        );
        assert_eq!(lexical_score("silver key", "The silver key turns."), 1.0);
        assert_eq!(lexical_score("", "anything"), 0.0);
    }

    #[test]
    fn limit_defaults_and_caps() {
        assert_eq!(normalize_limit(0), 50);
        assert_eq!(normalize_limit(10), 10);
        assert_eq!(normalize_limit(10_000), 1_000);
    }

    #[test]
    fn extracted_scores_are_normalized() {
        assert_eq!(normalize_importance(-1), 1);
        assert_eq!(normalize_importance(3), 3);
        assert_eq!(normalize_importance(9), 5);
        assert_eq!(normalize_max_attempts(0), 1);
        assert_eq!(normalize_max_attempts(3), 3);
        assert_eq!(normalize_max_attempts(50), 10);
        assert_eq!(normalize_confidence(-0.5), 0.0);
        assert_eq!(normalize_confidence(0.75), 0.75);
        assert_eq!(normalize_confidence(2.0), 1.0);
    }

    #[test]
    fn world_fact_content_is_derived_when_blank() {
        assert_eq!(
            world_fact_content(" silver key ", " opens ", " lower vault ", ""),
            "silver key opens lower vault"
        );
        assert_eq!(
            world_fact_content("silver key", "opens", "lower vault", "A custom fact."),
            "A custom fact."
        );
    }
}
