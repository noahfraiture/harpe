mod budget;
mod builder;
mod format;
mod token;
mod vector;

pub use budget::ContextBudget;
pub use builder::{ContextBuilder, ContextCandidate, ContextInputs, ContextKind};
pub use token::{TokenEstimator, TokenizerProfile, estimate_tokens};
pub use vector::cosine_similarity;

#[cfg(test)]
use crate::domain::{Character, Event, Location, Message, MessageRole, StorySummary, WorldFact};
#[cfg(test)]
use format::{format_location, format_world_fact, trusted_system_prompt};

#[cfg(test)]
include!("engine/tests.rs");
