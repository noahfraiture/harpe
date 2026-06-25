mod memory_update;
mod payload;
mod retry;
mod runner;

pub use memory_update::update_memory_after_turn;
pub use payload::{UpdateMemoryAfterTurnPayload, new_update_memory_job};
pub use runner::JobRunner;

#[cfg(test)]
use crate::HarpeError;
#[cfg(test)]
use crate::domain::{BackgroundJob, Character, JobKind, JobStatus, WorldFact};
#[cfg(test)]
use memory_update::{character_matches_fact, same_fact, same_name};
#[cfg(test)]
use retry::{retry_delay_for_attempt, should_retry};
#[cfg(test)]
use std::time::Duration;

#[cfg(test)]
include!("jobs/tests.rs");
