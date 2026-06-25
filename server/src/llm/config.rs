use std::time::Duration;

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
