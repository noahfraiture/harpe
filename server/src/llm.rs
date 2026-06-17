use std::pin::Pin;
use std::time::Duration;

use async_stream::try_stream;
use async_trait::async_trait;
use futures_core::Stream;
use futures_util::StreamExt;
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use tokio_stream::iter;

use crate::domain::{ExtractedEvent, MemoryExtraction, Message, MessageRole};
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HttpLlmConfig {
    pub base_url: String,
    pub api_key: Option<String>,
    pub chat_model: String,
    pub extraction_model: String,
    pub embedding_model: String,
    pub request_timeout: Duration,
    pub max_retries: usize,
    pub retry_base_delay: Duration,
}

impl HttpLlmConfig {
    pub fn openai_compatible(
        base_url: impl Into<String>,
        api_key: Option<String>,
        chat_model: impl Into<String>,
        extraction_model: impl Into<String>,
        embedding_model: impl Into<String>,
    ) -> Self {
        Self {
            base_url: base_url.into(),
            api_key,
            chat_model: chat_model.into(),
            extraction_model: extraction_model.into(),
            embedding_model: embedding_model.into(),
            request_timeout: Duration::from_secs(60),
            max_retries: 2,
            retry_base_delay: Duration::from_millis(200),
        }
    }

    pub fn with_request_policy(
        mut self,
        request_timeout: Duration,
        max_retries: usize,
        retry_base_delay: Duration,
    ) -> Self {
        self.request_timeout = request_timeout;
        self.max_retries = max_retries;
        self.retry_base_delay = retry_base_delay;
        self
    }
}

#[derive(Clone, Debug)]
pub struct HttpLlm {
    client: reqwest::Client,
    config: HttpLlmConfig,
}

impl HttpLlm {
    pub fn new(config: HttpLlmConfig) -> Result<Self> {
        if config.base_url.trim().is_empty() {
            return Err(HarpeError::Validation(
                "LLM base URL is required".to_owned(),
            ));
        }
        if config.chat_model.trim().is_empty() {
            return Err(HarpeError::Validation("chat model is required".to_owned()));
        }
        if config.extraction_model.trim().is_empty() {
            return Err(HarpeError::Validation(
                "extraction model is required".to_owned(),
            ));
        }
        if config.embedding_model.trim().is_empty() {
            return Err(HarpeError::Validation(
                "embedding model is required".to_owned(),
            ));
        }
        if config.request_timeout.is_zero() {
            return Err(HarpeError::Validation(
                "LLM request timeout must be greater than zero".to_owned(),
            ));
        }

        let mut config = config;
        config.base_url = config.base_url.trim_end_matches('/').to_owned();
        let client = reqwest::Client::builder()
            .timeout(config.request_timeout)
            .build()
            .map_err(llm_http_error)?;

        Ok(Self { client, config })
    }

    fn request(&self, path: &str) -> reqwest::RequestBuilder {
        let url = format!("{}/{}", self.config.base_url, path.trim_start_matches('/'));
        let request = self.client.post(url);

        if let Some(api_key) = self.config.api_key.as_deref()
            && !api_key.trim().is_empty()
        {
            return request.bearer_auth(api_key);
        }

        request
    }

    async fn send_json_with_retry<T: Serialize + Sync>(
        &self,
        path: &str,
        payload: &T,
    ) -> Result<reqwest::Response> {
        let mut attempt = 0_usize;

        loop {
            let result = self.request(path).json(payload).send().await;

            match result {
                Ok(response) => {
                    let status = response.status();
                    if should_retry_status(status) && attempt < self.config.max_retries {
                        self.sleep_before_retry(attempt).await;
                        attempt = attempt.saturating_add(1);
                        continue;
                    }

                    return Ok(response);
                }
                Err(error)
                    if should_retry_request_error(&error) && attempt < self.config.max_retries =>
                {
                    self.sleep_before_retry(attempt).await;
                    attempt = attempt.saturating_add(1);
                }
                Err(error) => return Err(llm_http_error(error)),
            }
        }
    }

    async fn sleep_before_retry(&self, attempt: usize) {
        let multiplier = 2_u32.saturating_pow(attempt.min(8) as u32);
        let delay = self.config.retry_base_delay.saturating_mul(multiplier);
        if !delay.is_zero() {
            tokio::time::sleep(delay).await;
        }
    }

    async fn complete_chat(&self, model: &str, messages: Vec<HttpChatMessage>) -> Result<String> {
        let payload = ChatCompletionRequest {
            model,
            messages,
            stream: false,
        };
        let response = self
            .send_json_with_retry("/v1/chat/completions", &payload)
            .await?;

        if !response.status().is_success() {
            return Err(response_error(response).await);
        }

        let completion: ChatCompletionResponse = response.json().await.map_err(llm_http_error)?;
        completion
            .choices
            .into_iter()
            .find_map(|choice| non_empty(choice.message.content))
            .ok_or_else(|| HarpeError::Llm("chat completion response was empty".to_owned()))
    }
}

#[async_trait]
impl LlmClient for HttpLlm {
    async fn stream_chat(&self, request: ChatRequest) -> Result<TextStream> {
        let payload = ChatCompletionRequest {
            model: &self.config.chat_model,
            messages: request
                .messages
                .into_iter()
                .map(HttpChatMessage::from_chat_message)
                .collect(),
            stream: true,
        };
        let response = self
            .send_json_with_retry("/v1/chat/completions", &payload)
            .await?;

        if !response.status().is_success() {
            return Err(response_error(response).await);
        }

        let stream = try_stream! {
            let mut byte_stream = response.bytes_stream();
            let mut buffer = String::new();

            while let Some(next) = byte_stream.next().await {
                let bytes = next.map_err(llm_http_error)?;
                let chunk = std::str::from_utf8(&bytes)
                    .map_err(|error| HarpeError::Llm(error.to_string()))?;
                buffer.push_str(chunk);

                while let Some((event_end, delimiter_len)) = next_sse_event(&buffer) {
                    let event = buffer[..event_end].to_owned();
                    buffer.drain(..event_end + delimiter_len);

                    let Some(data) = sse_data(&event) else {
                        continue;
                    };
                    if data == "[DONE]" {
                        return;
                    }

                    let chunk: ChatCompletionChunk = serde_json::from_str(&data)
                        .map_err(|error| HarpeError::Llm(error.to_string()))?;
                    for choice in chunk.choices {
                        if let Some(content) = non_empty(choice.delta.content) {
                            yield content;
                        }
                    }
                }
            }
        };

        Ok(Box::pin(stream))
    }

    async fn summarize(&self, request: SummarizeRequest) -> Result<String> {
        self.complete_chat(
            &self.config.chat_model,
            vec![
                HttpChatMessage {
                    role: "system",
                    content: "Update the roleplay story summary. Preserve durable plot facts, important consequences, and unresolved threads. Return only the summary text.".to_owned(),
                },
                HttpChatMessage {
                    role: "user",
                    content: format_summary_prompt(&request),
                },
            ],
        )
        .await
    }

    async fn extract_memory(&self, request: ExtractMemoryRequest) -> Result<MemoryExtraction> {
        let content = self
            .complete_chat(
                &self.config.extraction_model,
                vec![
                    HttpChatMessage {
                        role: "system",
                        content: "Extract durable roleplay game memory as strict JSON with keys events, character_updates, world_facts, and locations. Return JSON only.".to_owned(),
                    },
                    HttpChatMessage {
                        role: "user",
                        content: format_extraction_prompt(&request),
                    },
                ],
            )
            .await?;

        parse_memory_extraction(&content)
    }

    async fn embed(&self, text: &str) -> Result<Vec<f32>> {
        let payload = EmbeddingRequest {
            model: &self.config.embedding_model,
            input: text,
        };
        let response = self
            .send_json_with_retry("/v1/embeddings", &payload)
            .await?;

        if !response.status().is_success() {
            return Err(response_error(response).await);
        }

        let response: EmbeddingResponse = response.json().await.map_err(llm_http_error)?;
        response
            .data
            .into_iter()
            .next()
            .map(|item| item.embedding)
            .filter(|embedding| !embedding.is_empty())
            .ok_or_else(|| HarpeError::Llm("embedding response was empty".to_owned()))
    }
}

#[derive(Clone, Debug, Serialize)]
struct ChatCompletionRequest<'a> {
    model: &'a str,
    messages: Vec<HttpChatMessage>,
    stream: bool,
}

#[derive(Clone, Debug, Serialize)]
struct HttpChatMessage {
    role: &'static str,
    content: String,
}

impl HttpChatMessage {
    fn from_chat_message(message: ChatMessage) -> Self {
        Self {
            role: message.role.as_db_value(),
            content: message.content,
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
struct ChatCompletionResponse {
    choices: Vec<ChatCompletionChoice>,
}

#[derive(Clone, Debug, Deserialize)]
struct ChatCompletionChoice {
    message: ChatCompletionMessage,
}

#[derive(Clone, Debug, Deserialize)]
struct ChatCompletionMessage {
    content: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
struct ChatCompletionChunk {
    choices: Vec<ChatCompletionChunkChoice>,
}

#[derive(Clone, Debug, Deserialize)]
struct ChatCompletionChunkChoice {
    delta: ChatCompletionDelta,
}

#[derive(Clone, Debug, Deserialize)]
struct ChatCompletionDelta {
    content: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
struct EmbeddingRequest<'a> {
    model: &'a str,
    input: &'a str,
}

#[derive(Clone, Debug, Deserialize)]
struct EmbeddingResponse {
    data: Vec<EmbeddingData>,
}

#[derive(Clone, Debug, Deserialize)]
struct EmbeddingData {
    embedding: Vec<f32>,
}

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

fn extract_fallback_event(messages: Vec<Message>) -> MemoryExtraction {
    let Some(last_assistant_message) = messages
        .iter()
        .rev()
        .find(|message| message.role == MessageRole::Assistant)
    else {
        return MemoryExtraction::default();
    };

    let summary = first_sentence(&last_assistant_message.content);
    if summary.is_empty() {
        return MemoryExtraction::default();
    }

    MemoryExtraction {
        events: vec![ExtractedEvent {
            summary,
            importance: 3,
        }],
        ..MemoryExtraction::default()
    }
}

fn first_sentence(content: &str) -> String {
    let trimmed = content.trim();
    let end = trimmed
        .char_indices()
        .find_map(|(index, char)| {
            matches!(char, '.' | '!' | '?').then_some(index + char.len_utf8())
        })
        .unwrap_or(trimmed.len());

    trimmed[..end].trim().chars().take(240).collect()
}

fn format_summary_prompt(request: &SummarizeRequest) -> String {
    let mut prompt = String::new();

    if let Some(summary) = request.previous_summary.as_deref()
        && !summary.trim().is_empty()
    {
        prompt.push_str("Previous summary:\n");
        prompt.push_str(summary.trim());
        prompt.push_str("\n\n");
    }

    prompt.push_str("Recent transcript:\n");
    prompt.push_str(&format_transcript(&request.messages));
    prompt
}

fn format_extraction_prompt(request: &ExtractMemoryRequest) -> String {
    format!(
        "Game id: {}\nSession id: {}\n\nTranscript:\n{}",
        request.game_id,
        request.session_id,
        format_transcript(&request.messages)
    )
}

fn format_transcript(messages: &[Message]) -> String {
    messages
        .iter()
        .map(|message| format!("{}: {}", message.role.as_db_value(), message.content.trim()))
        .collect::<Vec<_>>()
        .join("\n")
}

fn parse_memory_extraction(content: &str) -> Result<MemoryExtraction> {
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return Err(HarpeError::Llm(
            "memory extraction response was empty".to_owned(),
        ));
    }

    if let Ok(extraction) = serde_json::from_str(trimmed) {
        return Ok(extraction);
    }

    if let Some(unfenced) = strip_code_fence(trimmed) {
        return serde_json::from_str(unfenced)
            .map_err(|error| HarpeError::Llm(format!("invalid memory extraction JSON: {error}")));
    }

    Err(HarpeError::Llm("invalid memory extraction JSON".to_owned()))
}

fn strip_code_fence(content: &str) -> Option<&str> {
    if !content.starts_with("```") {
        return None;
    }

    let open_end = content.find('\n')?;
    let body = &content[open_end + 1..];
    let close_start = body.rfind("```")?;

    Some(body[..close_start].trim())
}

fn next_sse_event(buffer: &str) -> Option<(usize, usize)> {
    match (buffer.find("\r\n\r\n"), buffer.find("\n\n")) {
        (Some(crlf), Some(lf)) if crlf < lf => Some((crlf, 4)),
        (Some(_), Some(lf)) => Some((lf, 2)),
        (Some(crlf), None) => Some((crlf, 4)),
        (None, Some(lf)) => Some((lf, 2)),
        (None, None) => None,
    }
}

fn sse_data(event: &str) -> Option<String> {
    let lines = event
        .lines()
        .filter_map(|line| line.trim_start().strip_prefix("data:"))
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>();

    (!lines.is_empty()).then(|| lines.join("\n"))
}

fn non_empty(value: Option<String>) -> Option<String> {
    value.and_then(|value| if value.is_empty() { None } else { Some(value) })
}

async fn response_error(response: reqwest::Response) -> HarpeError {
    let status = response.status();
    let body = response
        .text()
        .await
        .unwrap_or_else(|error| format!("failed to read error body: {error}"));

    HarpeError::Llm(format!("LLM HTTP {status}: {body}"))
}

fn should_retry_status(status: StatusCode) -> bool {
    status == StatusCode::TOO_MANY_REQUESTS || status.is_server_error()
}

fn should_retry_request_error(error: &reqwest::Error) -> bool {
    error.is_timeout() || error.is_connect()
}

fn llm_http_error(error: reqwest::Error) -> HarpeError {
    HarpeError::Llm(error.to_string())
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
    use chrono::Utc;
    use futures_util::StreamExt;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::{TcpListener, TcpStream};
    use tokio::sync::mpsc;

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

    #[test]
    fn http_llm_rejects_invalid_config() {
        let base =
            HttpLlmConfig::openai_compatible("http://localhost", None, "chat", "extract", "embed");

        assert!(matches!(
            HttpLlm::new(HttpLlmConfig {
                base_url: String::new(),
                ..base.clone()
            }),
            Err(HarpeError::Validation(_))
        ));
        assert!(matches!(
            HttpLlm::new(HttpLlmConfig {
                chat_model: String::new(),
                ..base.clone()
            }),
            Err(HarpeError::Validation(_))
        ));
        assert!(matches!(
            HttpLlm::new(HttpLlmConfig {
                extraction_model: String::new(),
                ..base.clone()
            }),
            Err(HarpeError::Validation(_))
        ));
        assert!(matches!(
            HttpLlm::new(HttpLlmConfig {
                embedding_model: String::new(),
                ..base.clone()
            }),
            Err(HarpeError::Validation(_))
        ));
        assert!(matches!(
            HttpLlm::new(HttpLlmConfig {
                request_timeout: Duration::ZERO,
                ..base
            }),
            Err(HarpeError::Validation(_))
        ));
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

    #[tokio::test]
    async fn extract_memory_uses_configured_fixture() {
        let llm = EchoLlm::development_default().with_extraction(MemoryExtraction {
            events: vec![ExtractedEvent {
                summary: "The sigil ignites.".to_owned(),
                importance: 4,
            }],
            ..MemoryExtraction::default()
        });

        let extraction = llm
            .extract_memory(ExtractMemoryRequest {
                game_id: "game-1".to_owned(),
                session_id: "session-1".to_owned(),
                messages: vec![],
            })
            .await
            .unwrap();

        assert_eq!(extraction.events[0].summary, "The sigil ignites.");
        assert_eq!(extraction.events[0].importance, 4);
    }

    #[tokio::test]
    async fn extract_memory_falls_back_to_last_assistant_event() {
        let llm = EchoLlm::development_default();
        let extraction = llm
            .extract_memory(ExtractMemoryRequest {
                game_id: "game-1".to_owned(),
                session_id: "session-1".to_owned(),
                messages: vec![Message {
                    id: "message-1".to_owned(),
                    session_id: "session-1".to_owned(),
                    role: MessageRole::Assistant,
                    content: "The old gate opens. A cold wind follows.".to_owned(),
                    created_at: Utc::now(),
                }],
            })
            .await
            .unwrap();

        assert_eq!(extraction.events[0].summary, "The old gate opens.");
        assert_eq!(extraction.events[0].importance, 3);
    }

    #[tokio::test]
    async fn extract_memory_fallback_ignores_missing_or_blank_assistant_messages() {
        let llm = EchoLlm::development_default();

        let no_assistant = llm
            .extract_memory(ExtractMemoryRequest {
                game_id: "game-1".to_owned(),
                session_id: "session-1".to_owned(),
                messages: vec![Message {
                    id: "message-1".to_owned(),
                    session_id: "session-1".to_owned(),
                    role: MessageRole::User,
                    content: "I wait.".to_owned(),
                    created_at: Utc::now(),
                }],
            })
            .await
            .unwrap();
        assert_eq!(no_assistant, MemoryExtraction::default());

        let blank_assistant = llm
            .extract_memory(ExtractMemoryRequest {
                game_id: "game-1".to_owned(),
                session_id: "session-1".to_owned(),
                messages: vec![Message {
                    id: "message-2".to_owned(),
                    session_id: "session-1".to_owned(),
                    role: MessageRole::Assistant,
                    content: "   ".to_owned(),
                    created_at: Utc::now(),
                }],
            })
            .await
            .unwrap();
        assert_eq!(blank_assistant, MemoryExtraction::default());
    }

    #[tokio::test]
    async fn http_llm_streams_openai_compatible_sse_chunks() {
        let sse = concat!(
            "data: {\"choices\":[{\"delta\":{\"content\":\"The gate \"}}]}\n\n",
            "data: {\"choices\":[{\"delta\":{\"content\":\"opens.\"}}]}\n\n",
            "data: [DONE]\n\n"
        );
        let mut mock = spawn_mock_http(vec![sse_response(sse)]).await;
        let llm = test_http_llm(&mock.base_url);
        let mut stream = llm
            .stream_chat(ChatRequest {
                messages: vec![ChatMessage {
                    role: MessageRole::User,
                    content: "I lift the latch.".to_owned(),
                }],
            })
            .await
            .unwrap();

        assert_eq!(stream.next().await.unwrap().unwrap(), "The gate ");
        assert_eq!(stream.next().await.unwrap().unwrap(), "opens.");
        assert!(stream.next().await.is_none());

        let request = mock.requests.recv().await.unwrap();
        assert!(request.contains("POST /v1/chat/completions"));
        assert!(request.contains("\"model\":\"chat-model\""));
        assert!(request.contains("\"stream\":true"));
        assert!(request.contains("\"content\":\"I lift the latch.\""));
    }

    #[tokio::test]
    async fn http_llm_embeds_with_configured_model() {
        let mut mock = spawn_mock_http(vec![json_response(
            r#"{"data":[{"embedding":[0.25,0.75]}]}"#,
        )])
        .await;
        let llm = test_http_llm(&mock.base_url);

        let embedding = llm.embed("silver key").await.unwrap();

        assert_eq!(embedding, vec![0.25, 0.75]);
        let request = mock.requests.recv().await.unwrap();
        assert!(request.contains("POST /v1/embeddings"));
        assert!(request.contains("\"model\":\"embedding-model\""));
        assert!(request.contains("\"input\":\"silver key\""));
    }

    #[tokio::test]
    async fn http_llm_reports_non_success_status() {
        let mut mock = spawn_mock_http(vec![http_response(
            "application/json",
            r#"{"error":"bad request"}"#,
            "400 Bad Request",
        )])
        .await;
        let llm = test_http_llm(&mock.base_url);

        let error = llm.embed("silver key").await.unwrap_err();

        assert!(matches!(error, HarpeError::Llm(message) if message.contains("400")));
        let request = mock.requests.recv().await.unwrap();
        assert!(request.contains("POST /v1/embeddings"));
    }

    #[tokio::test]
    async fn http_llm_retries_transient_status_before_success() {
        let mut mock = spawn_mock_http(vec![
            http_response(
                "application/json",
                r#"{"error":"temporarily overloaded"}"#,
                "503 Service Unavailable",
            ),
            json_response(r#"{"data":[{"embedding":[0.5,0.5]}]}"#),
        ])
        .await;
        let llm = test_http_llm_with_policy(&mock.base_url, 1, Duration::ZERO);

        let embedding = llm.embed("retryable input").await.unwrap();

        assert_eq!(embedding, vec![0.5, 0.5]);
        let first_request = mock.requests.recv().await.unwrap();
        let second_request = mock.requests.recv().await.unwrap();
        assert!(first_request.contains("POST /v1/embeddings"));
        assert!(second_request.contains("POST /v1/embeddings"));
    }

    #[tokio::test]
    async fn http_llm_does_not_retry_client_errors() {
        let mut mock = spawn_mock_http(vec![
            http_response(
                "application/json",
                r#"{"error":"bad request"}"#,
                "400 Bad Request",
            ),
            json_response(r#"{"data":[{"embedding":[1.0]}]}"#),
        ])
        .await;
        let llm = test_http_llm_with_policy(&mock.base_url, 2, Duration::ZERO);

        let error = llm.embed("bad input").await.unwrap_err();

        assert!(matches!(error, HarpeError::Llm(message) if message.contains("400")));
        let request = mock.requests.recv().await.unwrap();
        assert!(request.contains("POST /v1/embeddings"));
        assert!(
            tokio::time::timeout(Duration::from_millis(50), mock.requests.recv())
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn http_llm_rejects_empty_embedding_response() {
        let mock = spawn_mock_http(vec![json_response(r#"{"data":[]}"#)]).await;
        let llm = test_http_llm(&mock.base_url);

        let error = llm.embed("silver key").await.unwrap_err();

        assert!(matches!(error, HarpeError::Llm(message) if message.contains("empty")));
    }

    #[tokio::test]
    async fn http_llm_summarizes_and_extracts_memory_json() {
        let extraction_content = serde_json::json!({
            "events": [{"summary": "The sea gate opens.", "importance": 4}],
            "character_updates": [{"name": "Mira", "description": "Gate scout", "status": "alert"}],
            "world_facts": [{"subject": "sea gate", "predicate": "guards", "object": "harbor", "content": "The sea gate guards the harbor.", "confidence": 0.8}],
            "locations": [{"name": "Harbor", "description": "Storm-battered docks"}]
        })
        .to_string();
        let mut mock = spawn_mock_http(vec![
            chat_response("The party opened the sea gate."),
            chat_response(&extraction_content),
        ])
        .await;
        let llm = test_http_llm(&mock.base_url);
        let messages = vec![Message {
            id: "message-1".to_owned(),
            session_id: "session-1".to_owned(),
            role: MessageRole::Assistant,
            content: "The sea gate opens.".to_owned(),
            created_at: Utc::now(),
        }];

        let summary = llm
            .summarize(SummarizeRequest {
                previous_summary: Some("The party reached the harbor.".to_owned()),
                messages: messages.clone(),
            })
            .await
            .unwrap();
        let extraction = llm
            .extract_memory(ExtractMemoryRequest {
                game_id: "game-1".to_owned(),
                session_id: "session-1".to_owned(),
                messages,
            })
            .await
            .unwrap();

        assert_eq!(summary, "The party opened the sea gate.");
        assert_eq!(extraction.events[0].summary, "The sea gate opens.");
        assert_eq!(extraction.character_updates[0].name, "Mira");
        assert_eq!(
            extraction.world_facts[0].content,
            "The sea gate guards the harbor."
        );
        assert_eq!(extraction.locations[0].name, "Harbor");

        let summarize_request = mock.requests.recv().await.unwrap();
        let extraction_request = mock.requests.recv().await.unwrap();
        assert!(summarize_request.contains("\"model\":\"chat-model\""));
        assert!(summarize_request.contains("Previous summary"));
        assert!(extraction_request.contains("\"model\":\"extraction-model\""));
        assert!(extraction_request.contains("Game id: game-1"));
    }

    #[test]
    fn parses_fenced_memory_extraction_json() {
        let extraction = parse_memory_extraction(
            "```json\n{\"events\":[{\"summary\":\"A bell rings.\",\"importance\":2}]}\n```",
        )
        .unwrap();

        assert_eq!(extraction.events[0].summary, "A bell rings.");
        assert!(extraction.character_updates.is_empty());
    }

    #[test]
    fn memory_extraction_parser_rejects_empty_and_invalid_json() {
        assert!(matches!(
            parse_memory_extraction(" "),
            Err(HarpeError::Llm(message)) if message.contains("empty")
        ));
        assert!(matches!(
            parse_memory_extraction("not json"),
            Err(HarpeError::Llm(message)) if message.contains("invalid")
        ));
        assert!(strip_code_fence("{\"events\":[]}").is_none());
    }

    #[test]
    fn sse_event_parser_accepts_lf_and_crlf_delimiters() {
        assert_eq!(next_sse_event("data: one\n\nrest"), Some((9, 2)));
        assert_eq!(next_sse_event("data: one\r\n\r\nrest"), Some((9, 4)));
        assert_eq!(
            sse_data("event: ping\ndata: hello\n"),
            Some("hello".to_owned())
        );
    }

    struct MockHttp {
        base_url: String,
        requests: mpsc::Receiver<String>,
    }

    async fn spawn_mock_http(responses: Vec<String>) -> MockHttp {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let (tx, rx) = mpsc::channel(responses.len());

        tokio::spawn(async move {
            for response in responses {
                let (mut stream, _) = listener.accept().await.unwrap();
                let request = read_http_request(&mut stream).await;
                tx.send(request).await.unwrap();
                stream.write_all(response.as_bytes()).await.unwrap();
            }
        });

        MockHttp {
            base_url: format!("http://{addr}"),
            requests: rx,
        }
    }

    async fn read_http_request(stream: &mut TcpStream) -> String {
        let mut buffer = Vec::new();
        let mut temp = [0_u8; 1024];

        loop {
            let read = stream.read(&mut temp).await.unwrap();
            assert!(read > 0, "connection closed before request headers arrived");
            buffer.extend_from_slice(&temp[..read]);

            if let Some(header_end) = find_header_end(&buffer) {
                let content_length = content_length(&buffer[..header_end]);
                let required = header_end + content_length;
                while buffer.len() < required {
                    let read = stream.read(&mut temp).await.unwrap();
                    assert!(read > 0, "connection closed before request body arrived");
                    buffer.extend_from_slice(&temp[..read]);
                }
                break;
            }
        }

        String::from_utf8_lossy(&buffer).into_owned()
    }

    fn find_header_end(buffer: &[u8]) -> Option<usize> {
        buffer
            .windows(4)
            .position(|window| window == b"\r\n\r\n")
            .map(|index| index + 4)
    }

    fn content_length(headers: &[u8]) -> usize {
        let headers = String::from_utf8_lossy(headers);
        headers
            .lines()
            .find_map(|line| {
                let (name, value) = line.split_once(':')?;
                name.eq_ignore_ascii_case("content-length")
                    .then(|| value.trim().parse::<usize>().ok())
                    .flatten()
            })
            .unwrap_or(0)
    }

    fn test_http_llm(base_url: &str) -> HttpLlm {
        test_http_llm_with_policy(base_url, 2, Duration::from_millis(200))
    }

    fn test_http_llm_with_policy(
        base_url: &str,
        max_retries: usize,
        retry_base_delay: Duration,
    ) -> HttpLlm {
        HttpLlm::new(
            HttpLlmConfig::openai_compatible(
                base_url,
                Some("test-key".to_owned()),
                "chat-model",
                "extraction-model",
                "embedding-model",
            )
            .with_request_policy(Duration::from_secs(5), max_retries, retry_base_delay),
        )
        .unwrap()
    }

    fn chat_response(content: &str) -> String {
        json_response(
            &serde_json::json!({
                "choices": [{
                    "message": {
                        "content": content
                    }
                }]
            })
            .to_string(),
        )
    }

    fn json_response(body: &str) -> String {
        http_response("application/json", body, "200 OK")
    }

    fn sse_response(body: &str) -> String {
        http_response("text/event-stream", body, "200 OK")
    }

    fn http_response(content_type: &str, body: &str, status: &str) -> String {
        format!(
            "HTTP/1.1 {status}\r\ncontent-type: {content_type}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
            body.len()
        )
    }
}
