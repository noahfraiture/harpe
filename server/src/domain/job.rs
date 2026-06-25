use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum JobKind {
    UpdateMemoryAfterTurn,
}

impl JobKind {
    pub const fn as_db_value(self) -> &'static str {
        match self {
            Self::UpdateMemoryAfterTurn => "update_memory_after_turn",
        }
    }

    pub fn from_db_value(value: &str) -> Option<Self> {
        match value {
            "update_memory_after_turn" => Some(Self::UpdateMemoryAfterTurn),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum JobStatus {
    Pending,
    Running,
    Succeeded,
    Failed,
}

impl JobStatus {
    pub const fn as_db_value(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Running => "running",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
        }
    }

    pub fn from_db_value(value: &str) -> Option<Self> {
        match value {
            "pending" => Some(Self::Pending),
            "running" => Some(Self::Running),
            "succeeded" => Some(Self::Succeeded),
            "failed" => Some(Self::Failed),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BackgroundJob {
    pub id: String,
    pub kind: JobKind,
    pub status: JobStatus,
    pub payload: Value,
    pub attempts: i32,
    pub max_attempts: i32,
    pub last_error: Option<String>,
    pub run_after: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct NewBackgroundJob {
    pub kind: JobKind,
    pub payload: Value,
    pub max_attempts: i32,
    pub run_after: Option<DateTime<Utc>>,
}
