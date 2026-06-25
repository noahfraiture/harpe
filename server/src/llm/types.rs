use std::pin::Pin;

use async_trait::async_trait;
use futures_core::Stream;

use crate::Result;
use crate::domain::{MemoryExtraction, Message, MessageRole};

pub type TextStream = Pin<Box<dyn Stream<Item = Result<String>> + Send>>;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChatMessage {
    pub role: MessageRole,
    pub content: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChatRequest {
    pub messages: Vec<ChatMessage>,
    pub model: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SummarizeRequest {
    pub previous_summary: Option<String>,
    pub messages: Vec<Message>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExtractMemoryRequest {
    pub game_id: String,
    pub session_id: String,
    pub messages: Vec<Message>,
}

#[async_trait]
pub trait LlmClient: Send + Sync {
    async fn stream_chat(&self, request: ChatRequest) -> Result<TextStream>;
    async fn summarize(&self, request: SummarizeRequest) -> Result<String>;
    async fn extract_memory(&self, request: ExtractMemoryRequest) -> Result<MemoryExtraction>;
    async fn embed(&self, text: &str) -> Result<Vec<f32>>;
}
