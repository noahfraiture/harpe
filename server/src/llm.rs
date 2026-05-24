use std::pin::Pin;

use async_trait::async_trait;
use futures_core::Stream;
use tokio_stream::iter;

use crate::domain::{Message, MessageRole};
use crate::{HarpeError, Result};

pub type TextStream = Pin<Box<dyn Stream<Item = Result<String>> + Send>>;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChatMessage {
    pub role: MessageRole,
    pub content: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChatRequest {
    pub messages: Vec<ChatMessage>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SummarizeRequest {
    pub previous_summary: Option<String>,
    pub messages: Vec<Message>,
}

#[async_trait]
pub trait LlmClient: Send + Sync {
    async fn stream_chat(&self, request: ChatRequest) -> Result<TextStream>;
    async fn summarize(&self, request: SummarizeRequest) -> Result<String>;
    async fn embed(&self, text: &str) -> Result<Vec<f32>>;
}

#[derive(Clone, Debug)]
pub struct EchoLlm {
    response_chunks: Vec<String>,
}

impl EchoLlm {
    pub fn new(response_chunks: Vec<String>) -> Self {
        Self { response_chunks }
    }

    pub fn development_default() -> Self {
        Self::new(vec!["The story continues.".to_owned()])
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

    async fn embed(&self, text: &str) -> Result<Vec<f32>> {
        Ok(stable_embedding(text, 16))
    }
}

fn stable_embedding(text: &str, dimensions: usize) -> Vec<f32> {
    let mut embedding = vec![0.0; dimensions];

    for (index, byte) in text.bytes().enumerate() {
        let slot = index % dimensions;
        embedding[slot] += f32::from(byte) / 255.0;
    }

    let norm = embedding
        .iter()
        .map(|value| value * value)
        .sum::<f32>()
        .sqrt();

    if norm > 0.0 {
        for value in &mut embedding {
            *value /= norm;
        }
    }

    embedding
}

#[cfg(test)]
mod tests {
    use futures_util::StreamExt;

    use super::*;

    #[tokio::test]
    async fn echo_llm_streams_configured_chunks() {
        let llm = EchoLlm::new(vec!["one".to_owned(), " two".to_owned()]);
        let mut stream = llm
            .stream_chat(ChatRequest { messages: vec![] })
            .await
            .unwrap();

        assert_eq!(stream.next().await.unwrap().unwrap(), "one");
        assert_eq!(stream.next().await.unwrap().unwrap(), " two");
        assert!(stream.next().await.is_none());
    }

    #[tokio::test]
    async fn echo_llm_embedding_is_stable_and_normalized() {
        let llm = EchoLlm::development_default();

        let first = llm.embed("silver key").await.unwrap();
        let second = llm.embed("silver key").await.unwrap();
        let magnitude = first.iter().map(|value| value * value).sum::<f32>().sqrt();

        assert_eq!(first, second);
        assert!((magnitude - 1.0).abs() < 0.001);
    }

    #[tokio::test]
    async fn summarize_requires_content() {
        let llm = EchoLlm::development_default();
        let err = llm
            .summarize(SummarizeRequest {
                previous_summary: None,
                messages: vec![],
            })
            .await
            .unwrap_err();

        assert!(matches!(err, HarpeError::Llm(_)));
    }
}
