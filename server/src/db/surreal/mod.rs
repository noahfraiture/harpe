use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use surrealdb::Surreal;
use surrealdb::engine::any::{self, Any};
use surrealdb::opt::auth::Root;
use surrealdb::types::ToSql;

use crate::domain::{
    BackgroundJob, Character, Event, Game, GraphEdge, GraphRelationKind, JobStatus, Location,
    MemoryChunk, MemoryHit, Message, NewBackgroundJob, NewEvent, NewGame, NewMemoryChunk,
    NewMessage, NewSession, NewUser, Session, StorySummary, UpsertCharacter, UpsertLocation,
    UpsertStorySummary, UpsertWorldFact, User, WorldFact, new_id,
};
use crate::engine::cosine_similarity;
use crate::store::HarpeStore;
use crate::{HarpeError, Result};

mod migrations;
mod normalize;
mod relations;
mod rows;
mod search;
mod validation;

use migrations::{AppliedMigration, MIGRATION_BOOTSTRAP, MIGRATIONS, Migration};
use normalize::{
    memory_candidate_limit, normalize_confidence, normalize_importance, normalize_limit,
    normalize_max_attempts, world_fact_content,
};
#[cfg(test)]
use relations::RelationSpec;
use relations::{edge_id, relation_spec};
use rows::{
    CharacterRow, EventRow, GameRow, GraphEdgeRow, JobRow, LocationRow, MemoryChunkRow,
    MemorySearchCandidate, MemorySearchRow, MessageRow, MigrationRow, SessionRow, SummaryRow,
    UserRow, WorldFactRow,
};
use search::{fixed_embedding, indexed_embedding_field, lexical_score};
use validation::validate_present;

#[derive(Clone)]
pub struct SurrealStore {
    db: Arc<Surreal<Any>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SurrealCredentials {
    pub username: String,
    pub password: String,
}

impl SurrealStore {
    pub async fn connect(
        endpoint: impl Into<String>,
        namespace: &str,
        database: &str,
    ) -> Result<Self> {
        Self::connect_with_credentials(endpoint, namespace, database, None).await
    }

    pub async fn connect_with_credentials(
        endpoint: impl Into<String>,
        namespace: &str,
        database: &str,
        credentials: Option<SurrealCredentials>,
    ) -> Result<Self> {
        let db = any::connect(endpoint.into()).await?;
        if let Some(credentials) = credentials {
            db.signin(Root {
                username: credentials.username,
                password: credentials.password,
            })
            .await?;
        }
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
        run_after: Option<DateTime<Utc>>,
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
        if let Some(run_after) = run_after {
            row.run_after = run_after;
        }
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
            None,
        )
        .await
        .map(Some)
    }

    async fn complete_job(&self, job_id: &str) -> Result<BackgroundJob> {
        self.update_job_state(job_id, JobStatus::Succeeded, None, None, None)
            .await
    }

    async fn retry_job(
        &self,
        job_id: &str,
        error: String,
        run_after: DateTime<Utc>,
    ) -> Result<BackgroundJob> {
        self.update_job_state(
            job_id,
            JobStatus::Pending,
            None,
            Some(error),
            Some(run_after),
        )
        .await
    }

    async fn fail_job(&self, job_id: &str, error: String) -> Result<BackgroundJob> {
        self.update_job_state(job_id, JobStatus::Failed, None, Some(error), None)
            .await
    }

    async fn retry_failed_job(
        &self,
        job_id: &str,
        max_attempts: Option<i32>,
    ) -> Result<BackgroundJob> {
        let mut row: JobRow = self
            .db
            .select(("background_job", job_id))
            .await?
            .ok_or_else(|| HarpeError::NotFound(format!("background job {job_id}")))?;
        let status = JobStatus::from_db_value(&row.status)
            .ok_or_else(|| HarpeError::Store(format!("unknown job status {}", row.status)))?;
        if status != JobStatus::Failed {
            return Err(HarpeError::Validation(format!(
                "background job {job_id} is not failed"
            )));
        }

        row.status = JobStatus::Pending.as_db_value().to_owned();
        row.attempts = 0;
        if let Some(max_attempts) = max_attempts {
            row.max_attempts = normalize_max_attempts(max_attempts);
        }
        row.last_error = None;
        row.run_after = Utc::now();
        row.updated_at = Utc::now();

        let updated: Option<JobRow> = self
            .db
            .update(("background_job", row.uid.as_str()))
            .content(row)
            .await?;

        updated
            .map(TryInto::try_into)
            .transpose()?
            .ok_or_else(|| HarpeError::Store("SurrealDB did not return retried job".to_owned()))
    }

    async fn purge_failed_job(&self, job_id: &str) -> Result<BackgroundJob> {
        let row: JobRow = self
            .db
            .select(("background_job", job_id))
            .await?
            .ok_or_else(|| HarpeError::NotFound(format!("background job {job_id}")))?;
        let status = JobStatus::from_db_value(&row.status)
            .ok_or_else(|| HarpeError::Store(format!("unknown job status {}", row.status)))?;
        if status != JobStatus::Failed {
            return Err(HarpeError::Validation(format!(
                "background job {job_id} is not failed"
            )));
        }

        let deleted: Option<JobRow> = self.db.delete(("background_job", job_id)).await?;

        deleted
            .map(TryInto::try_into)
            .transpose()?
            .ok_or_else(|| HarpeError::Store("SurrealDB did not return purged job".to_owned()))
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

    async fn upsert_graph_edge(
        &self,
        relation: GraphRelationKind,
        in_record_id: &str,
        out_record_id: &str,
    ) -> Result<()> {
        validate_present("graph edge source id", in_record_id)?;
        validate_present("graph edge target id", out_record_id)?;

        self.upsert_graph_relation(relation, in_record_id, out_record_id)
            .await
    }

    async fn save_memory_chunk(&self, input: NewMemoryChunk) -> Result<MemoryHit> {
        validate_present("session id", &input.session_id)?;
        validate_present("memory content", &input.content)?;

        let row = MemoryChunkRow {
            uid: new_id(),
            session_id: input.session_id,
            kind: input.kind,
            content: input.content,
            embedding_16: fixed_embedding(&input.embedding, 16),
            embedding_384: fixed_embedding(&input.embedding, 384),
            embedding_768: fixed_embedding(&input.embedding, 768),
            embedding_1024: fixed_embedding(&input.embedding, 1024),
            embedding_1536: fixed_embedding(&input.embedding, 1536),
            embedding_3072: fixed_embedding(&input.embedding, 3072),
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
        let limit = normalize_limit(limit);
        let candidate_limit = memory_candidate_limit(limit);
        let query = query.trim();
        let mut candidates = Vec::new();

        if let Some(indexed_field) = indexed_embedding_field(query_embedding.len()) {
            let vector_query = format!(
                "SELECT * FROM memory_chunk
                 WHERE session_id = $session_id
                   AND {indexed_field} != NONE
                   AND {indexed_field} <|{candidate_limit},100|> $query_embedding
                 LIMIT {candidate_limit}",
            );
            let mut response = self
                .db
                .query(vector_query)
                .bind(("session_id", session_id.to_owned()))
                .bind(("query_embedding", query_embedding.to_vec()))
                .await?;
            let rows: Vec<MemoryChunkRow> = response.take(0)?;
            candidates.extend(rows.into_iter().map(Into::into));
        }

        if !query.is_empty() {
            let mut response = self
                .db
                .query(
                    "SELECT *, search::score(1) AS lexical_score
                     FROM memory_chunk
                     WHERE session_id = $session_id
                       AND content @1@ $query
                     ORDER BY lexical_score DESC
                     LIMIT $candidate_limit",
                )
                .bind(("session_id", session_id.to_owned()))
                .bind(("query", query.to_owned()))
                .bind(("candidate_limit", candidate_limit as i64))
                .await?;
            let rows: Vec<MemorySearchRow> = response.take(0)?;
            candidates.extend(rows.into_iter().map(Into::into));
        }

        if candidates.is_empty() {
            let mut response = self
                .db
                .query(
                    "SELECT * FROM memory_chunk
                     WHERE session_id = $session_id
                     ORDER BY created_at DESC
                     LIMIT $candidate_limit",
                )
                .bind(("session_id", session_id.to_owned()))
                .bind(("candidate_limit", candidate_limit as i64))
                .await?;
            let rows: Vec<MemoryChunkRow> = response.take(0)?;
            candidates.extend(rows.into_iter().map(Into::into));
        }

        Ok(rank_memory_candidates(
            candidates,
            query,
            query_embedding,
            limit,
        ))
    }
}

fn migration_id(migration: Migration) -> String {
    format!("m{:04}_{}", migration.version, migration.name)
}

fn rank_memory_candidates(
    candidates: Vec<MemorySearchCandidate>,
    query: &str,
    query_embedding: &[f32],
    limit: usize,
) -> Vec<MemoryHit> {
    let mut hits = candidates
        .into_iter()
        .fold(Vec::<MemoryHit>::new(), |mut hits, candidate| {
            let vector_score = cosine_similarity(query_embedding, &candidate.row.embedding);
            let lexical_score = candidate
                .lexical_score
                .unwrap_or_else(|| lexical_score(&query.to_lowercase(), &candidate.row.content));
            let score = vector_score.max(lexical_score);
            let chunk: MemoryChunk = candidate.row.into();

            if let Some(existing) = hits.iter_mut().find(|hit| hit.chunk.id == chunk.id) {
                existing.score = existing.score.max(score);
            } else if score > 0.0 || query.is_empty() {
                hits.push(MemoryHit { chunk, score });
            }

            hits
        });

    hits.sort_by(|left, right| {
        right
            .score
            .total_cmp(&left.score)
            .then_with(|| right.chunk.created_at.cmp(&left.chunk.created_at))
    });
    hits.truncate(normalize_limit(limit));
    hits
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
        assert_eq!(memory_candidate_limit(1), 32);
        assert_eq!(memory_candidate_limit(100), 512);
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

    #[test]
    fn fixed_embedding_is_only_populated_for_indexed_dimension() {
        assert_eq!(indexed_embedding_field(16), Some("embedding_16"));
        assert_eq!(indexed_embedding_field(1536), Some("embedding_1536"));
        assert_eq!(indexed_embedding_field(17), None);
        assert_eq!(fixed_embedding(&[1.0; 15], 16), None);
        assert_eq!(fixed_embedding(&[1.0; 17], 16), None);
        assert_eq!(fixed_embedding(&[1.0; 16], 16), Some(vec![1.0; 16]));
    }

    #[test]
    fn memory_candidate_ranking_deduplicates_and_prefers_best_score() {
        let now = Utc::now();
        let rows = vec![
            MemorySearchCandidate {
                row: MemoryChunkRow {
                    uid: "memory-1".to_owned(),
                    session_id: "session-1".to_owned(),
                    kind: "event".to_owned(),
                    content: "The silver key opens the lower vault.".to_owned(),
                    embedding_16: Some(vec![1.0; 16]),
                    embedding_384: None,
                    embedding_768: None,
                    embedding_1024: None,
                    embedding_1536: None,
                    embedding_3072: None,
                    embedding: vec![1.0, 0.0],
                    created_at: now,
                },
                lexical_score: Some(0.2),
            },
            MemorySearchCandidate {
                row: MemoryChunkRow {
                    uid: "memory-1".to_owned(),
                    session_id: "session-1".to_owned(),
                    kind: "event".to_owned(),
                    content: "The silver key opens the lower vault.".to_owned(),
                    embedding_16: Some(vec![1.0; 16]),
                    embedding_384: None,
                    embedding_768: None,
                    embedding_1024: None,
                    embedding_1536: None,
                    embedding_3072: None,
                    embedding: vec![1.0, 0.0],
                    created_at: now,
                },
                lexical_score: Some(0.9),
            },
            MemorySearchCandidate {
                row: MemoryChunkRow {
                    uid: "memory-2".to_owned(),
                    session_id: "session-1".to_owned(),
                    kind: "event".to_owned(),
                    content: "Unrelated memory.".to_owned(),
                    embedding_16: None,
                    embedding_384: None,
                    embedding_768: None,
                    embedding_1024: None,
                    embedding_1536: None,
                    embedding_3072: None,
                    embedding: vec![0.0, 1.0],
                    created_at: now,
                },
                lexical_score: Some(0.1),
            },
        ];

        let hits = rank_memory_candidates(rows, "silver key", &[1.0, 0.0], 10);

        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].chunk.id, "memory-1");
        assert!((hits[0].score - 1.0).abs() < 0.001);
    }

    #[test]
    fn relation_specs_cover_extraction_edge_tables() {
        assert_eq!(
            relation_spec(GraphRelationKind::EventInvolvesCharacter),
            RelationSpec {
                table: "event_involves_character",
                in_table: "event",
                out_table: "character",
            }
        );
        assert_eq!(
            relation_spec(GraphRelationKind::EventHappenedAtLocation),
            RelationSpec {
                table: "event_happened_at_location",
                in_table: "event",
                out_table: "location",
            }
        );
        assert_eq!(
            relation_spec(GraphRelationKind::CharacterKnowsWorldFact),
            RelationSpec {
                table: "character_knows_world_fact",
                in_table: "character",
                out_table: "world_fact",
            }
        );
        assert_eq!(
            relation_spec(GraphRelationKind::MemorySupportsWorldFact),
            RelationSpec {
                table: "memory_supports_world_fact",
                in_table: "memory_chunk",
                out_table: "world_fact",
            }
        );
    }
}
