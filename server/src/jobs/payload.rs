use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::domain::{JobKind, NewBackgroundJob};
use crate::{HarpeError, Result};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct UpdateMemoryAfterTurnPayload {
    pub game_id: String,
    pub session_id: String,
    pub assistant_message_id: String,
    pub assistant_content: String,
}

impl UpdateMemoryAfterTurnPayload {
    pub fn new(
        game_id: String,
        session_id: String,
        assistant_message_id: String,
        assistant_content: String,
    ) -> Self {
        Self {
            game_id,
            session_id,
            assistant_message_id,
            assistant_content,
        }
    }

    pub fn into_value(self) -> Result<Value> {
        serde_json::to_value(self).map_err(|error| HarpeError::Store(error.to_string()))
    }

    pub fn from_value(value: Value) -> Result<Self> {
        serde_json::from_value(value).map_err(|error| HarpeError::Validation(error.to_string()))
    }
}

pub fn new_update_memory_job(payload: UpdateMemoryAfterTurnPayload) -> Result<NewBackgroundJob> {
    Ok(NewBackgroundJob {
        kind: JobKind::UpdateMemoryAfterTurn,
        payload: payload.into_value()?,
        max_attempts: 3,
        run_after: None,
    })
}
