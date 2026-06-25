use async_trait::async_trait;
use tokio_stream::iter;

use crate::domain::{MemoryExtraction, MessageRole};
use crate::{HarpeError, Result};

use super::embedding::stable_embedding;
use super::extraction::extract_fallback_event;
use super::types::{ChatRequest, ExtractMemoryRequest, LlmClient, SummarizeRequest, TextStream};

#[derive(Clone, Debug)]
pub struct EchoLlm {
    response_chunks: Vec<String>,
    extraction: Option<MemoryExtraction>,
}

impl EchoLlm {
    pub fn new(response_chunks: Vec<String>) -> Self {
        Self {
            response_chunks,
            extraction: None,
        }
    }

    pub fn development_default() -> Self {
        Self::new(vec!["The story continues.".to_owned()])
    }

    pub fn with_extraction(mut self, extraction: MemoryExtraction) -> Self {
        self.extraction = Some(extraction);
        self
    }
}

#[async_trait]
impl LlmClient for EchoLlm {
    async fn stream_chat(&self, request: ChatRequest) -> Result<TextStream> {
        if self.response_chunks.is_empty() {
            let last_user = request
                .messages
                .iter()
                .rev()
                .find(|message| message.role == MessageRole::User)
                .map(|message| message.content.as_str())
                .unwrap_or("the player acts");

            let response = format!("Narrator: {last_user}");
            return Ok(Box::pin(iter(vec![Ok(response)])));
        }

        let chunks = self
            .response_chunks
            .iter()
            .cloned()
            .map(Ok)
            .collect::<Vec<_>>();
        Ok(Box::pin(iter(chunks)))
    }

    async fn summarize(&self, request: SummarizeRequest) -> Result<String> {
        let mut parts = Vec::new();

        if let Some(summary) = request.previous_summary
            && !summary.trim().is_empty()
        {
            parts.push(summary);
        }

        let recent = request
            .messages
            .iter()
            .rev()
            .take(4)
            .map(|message| format!("{}: {}", message.role.as_db_value(), message.content))
            .collect::<Vec<_>>();

        parts.extend(recent.into_iter().rev());

        if parts.is_empty() {
            return Err(HarpeError::Llm(
                "cannot summarize an empty conversation".to_owned(),
            ));
        }

        Ok(parts.join("\n"))
    }

    async fn extract_memory(&self, request: ExtractMemoryRequest) -> Result<MemoryExtraction> {
        if let Some(extraction) = &self.extraction {
            return Ok(extraction.clone());
        }

        Ok(extract_fallback_event(request.messages))
    }

    async fn embed(&self, text: &str) -> Result<Vec<f32>> {
        Ok(stable_embedding(text, 16))
    }
}
