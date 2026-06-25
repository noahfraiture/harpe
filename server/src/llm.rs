mod config;
mod echo;
mod embedding;
mod extraction;
mod openai;
mod prompts;
mod sse;
mod types;

pub use config::HttpLlmConfig;
pub use echo::EchoLlm;
pub use openai::HttpLlm;
pub use types::{
    ChatMessage, ChatRequest, ExtractMemoryRequest, LlmClient, SummarizeRequest, TextStream,
};

#[cfg(test)]
use crate::HarpeError;
#[cfg(test)]
use crate::domain::{ExtractedEvent, MemoryExtraction, Message, MessageRole};
#[cfg(test)]
use extraction::{parse_memory_extraction, strip_code_fence};
#[cfg(test)]
use sse::{next_sse_event, sse_data};
#[cfg(test)]
use std::time::Duration;

#[cfg(test)]
include!("llm/tests.rs");
