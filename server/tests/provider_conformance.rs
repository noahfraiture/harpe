use std::time::Duration;

use futures_util::StreamExt;
use harpe_server::domain::MessageRole;
use harpe_server::llm::{ChatMessage, ChatRequest, HttpLlm, HttpLlmConfig, LlmClient};

#[tokio::test]
#[ignore = "requires a real OpenAI-compatible provider; set HARPE_PROVIDER_CONFORMANCE_* env vars"]
async fn openai_compatible_provider_streams_chat_and_embeds() {
    let llm = HttpLlm::new(
        HttpLlmConfig::openai_compatible(
            required_env("HARPE_PROVIDER_CONFORMANCE_BASE_URL"),
            std::env::var("HARPE_PROVIDER_CONFORMANCE_API_KEY").ok(),
            required_env("HARPE_PROVIDER_CONFORMANCE_CHAT_MODEL"),
            required_env("HARPE_PROVIDER_CONFORMANCE_EXTRACTION_MODEL"),
            required_env("HARPE_PROVIDER_CONFORMANCE_EMBEDDING_MODEL"),
        )
        .with_request_policy(Duration::from_secs(45), 1, Duration::from_millis(250)),
    )
    .unwrap();

    let mut stream = llm
        .stream_chat(ChatRequest {
            messages: vec![
                ChatMessage {
                    role: MessageRole::System,
                    content: "Reply with a short confirmation containing the word harpe."
                        .to_owned(),
                },
                ChatMessage {
                    role: MessageRole::User,
                    content: "Confirm provider conformance.".to_owned(),
                },
            ],
            model: None,
        })
        .await
        .unwrap();
    let mut response = String::new();
    while let Some(delta) = stream.next().await {
        response.push_str(&delta.unwrap());
    }

    assert!(
        response.to_ascii_lowercase().contains("harpe"),
        "provider response did not follow the conformance prompt: {response:?}"
    );

    let embedding = llm.embed("harpe provider conformance").await.unwrap();
    assert!(!embedding.is_empty());
    assert!(embedding.iter().all(|value| value.is_finite()));
}

fn required_env(key: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| panic!("{key} is required"))
}
