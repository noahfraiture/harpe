use std::net::SocketAddr;
use std::time::Duration;

use crate::llm::HttpLlmConfig;
use crate::{HarpeError, Result};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AppConfig {
    pub grpc_addr: SocketAddr,
    pub surreal_endpoint: String,
    pub surreal_namespace: String,
    pub surreal_database: String,
    pub llm: AppLlmConfig,
    pub job_interval: Duration,
    pub job_batch_limit: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AppLlmConfig {
    Echo,
    Http(HttpLlmConfig),
}

impl AppConfig {
    pub fn from_env() -> Result<Self> {
        Self::from_vars(|key| std::env::var(key).ok())
    }

    pub fn from_vars(mut var: impl FnMut(&str) -> Option<String>) -> Result<Self> {
        let grpc_addr = parse_socket_addr(
            &var("HARPE_GRPC_ADDR").unwrap_or_else(|| "[::1]:50051".to_owned()),
            "HARPE_GRPC_ADDR",
        )?;
        let surreal_endpoint = var("SURREALDB_ENDPOINT").unwrap_or_else(|| "memory".to_owned());
        let surreal_namespace = var("SURREALDB_NAMESPACE").unwrap_or_else(|| "harpe".to_owned());
        let surreal_database = var("SURREALDB_DATABASE").unwrap_or_else(|| "dev".to_owned());
        let llm = match var("HARPE_LLM_PROVIDER")
            .unwrap_or_else(|| "echo".to_owned())
            .trim()
            .to_ascii_lowercase()
            .as_str()
        {
            "echo" => AppLlmConfig::Echo,
            "http" | "openai-compatible" => AppLlmConfig::Http(
                HttpLlmConfig::openai_compatible(
                    required_var(&mut var, "HARPE_LLM_BASE_URL")?,
                    var("HARPE_LLM_API_KEY"),
                    required_var(&mut var, "HARPE_LLM_CHAT_MODEL")?,
                    required_var(&mut var, "HARPE_LLM_EXTRACTION_MODEL")?,
                    required_var(&mut var, "HARPE_LLM_EMBEDDING_MODEL")?,
                )
                .with_request_policy(
                    Duration::from_millis(parse_u64(
                        var("HARPE_LLM_TIMEOUT_MS")
                            .unwrap_or_else(|| "60000".to_owned())
                            .as_str(),
                        "HARPE_LLM_TIMEOUT_MS",
                    )?),
                    parse_usize(
                        var("HARPE_LLM_MAX_RETRIES")
                            .unwrap_or_else(|| "2".to_owned())
                            .as_str(),
                        "HARPE_LLM_MAX_RETRIES",
                    )?,
                    Duration::from_millis(parse_u64(
                        var("HARPE_LLM_RETRY_BASE_MS")
                            .unwrap_or_else(|| "200".to_owned())
                            .as_str(),
                        "HARPE_LLM_RETRY_BASE_MS",
                    )?),
                ),
            ),
            provider => {
                return Err(HarpeError::Validation(format!(
                    "unsupported HARPE_LLM_PROVIDER {provider}"
                )));
            }
        };

        Ok(Self {
            grpc_addr,
            surreal_endpoint,
            surreal_namespace,
            surreal_database,
            llm,
            job_interval: Duration::from_millis(parse_u64(
                var("HARPE_JOB_INTERVAL_MS")
                    .unwrap_or_else(|| "2000".to_owned())
                    .as_str(),
                "HARPE_JOB_INTERVAL_MS",
            )?),
            job_batch_limit: parse_usize(
                var("HARPE_JOB_BATCH_LIMIT")
                    .unwrap_or_else(|| "25".to_owned())
                    .as_str(),
                "HARPE_JOB_BATCH_LIMIT",
            )?,
        })
    }
}

fn required_var(var: &mut impl FnMut(&str) -> Option<String>, key: &str) -> Result<String> {
    let value = var(key).unwrap_or_default();
    if value.trim().is_empty() {
        return Err(HarpeError::Validation(format!("{key} is required")));
    }

    Ok(value)
}

fn parse_socket_addr(value: &str, key: &str) -> Result<SocketAddr> {
    value
        .parse()
        .map_err(|error| HarpeError::Validation(format!("{key} is invalid: {error}")))
}

fn parse_u64(value: &str, key: &str) -> Result<u64> {
    value
        .parse()
        .map_err(|error| HarpeError::Validation(format!("{key} is invalid: {error}")))
}

fn parse_usize(value: &str, key: &str) -> Result<usize> {
    value
        .parse()
        .map_err(|error| HarpeError::Validation(format!("{key} is invalid: {error}")))
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    #[test]
    fn config_uses_safe_development_defaults() {
        let config = AppConfig::from_vars(|_| None).unwrap();

        assert_eq!(config.grpc_addr.to_string(), "[::1]:50051");
        assert_eq!(config.surreal_endpoint, "memory");
        assert_eq!(config.surreal_namespace, "harpe");
        assert_eq!(config.surreal_database, "dev");
        assert_eq!(config.llm, AppLlmConfig::Echo);
        assert_eq!(config.job_interval, Duration::from_secs(2));
        assert_eq!(config.job_batch_limit, 25);
    }

    #[test]
    fn config_builds_http_llm_settings_from_vars() {
        let vars = HashMap::from([
            ("HARPE_GRPC_ADDR", "127.0.0.1:50051"),
            ("HARPE_LLM_PROVIDER", "http"),
            ("HARPE_LLM_BASE_URL", "http://localhost:11434"),
            ("HARPE_LLM_API_KEY", "secret"),
            ("HARPE_LLM_CHAT_MODEL", "chat"),
            ("HARPE_LLM_EXTRACTION_MODEL", "extract"),
            ("HARPE_LLM_EMBEDDING_MODEL", "embed"),
            ("HARPE_LLM_TIMEOUT_MS", "15000"),
            ("HARPE_LLM_MAX_RETRIES", "4"),
            ("HARPE_LLM_RETRY_BASE_MS", "25"),
            ("HARPE_JOB_INTERVAL_MS", "500"),
            ("HARPE_JOB_BATCH_LIMIT", "7"),
        ]);

        let config = AppConfig::from_vars(|key| vars.get(key).map(ToString::to_string)).unwrap();

        assert_eq!(config.grpc_addr.to_string(), "127.0.0.1:50051");
        assert_eq!(config.job_interval, Duration::from_millis(500));
        assert_eq!(config.job_batch_limit, 7);
        assert_eq!(
            config.llm,
            AppLlmConfig::Http(
                HttpLlmConfig::openai_compatible(
                    "http://localhost:11434",
                    Some("secret".to_owned()),
                    "chat",
                    "extract",
                    "embed",
                )
                .with_request_policy(
                    Duration::from_millis(15000),
                    4,
                    Duration::from_millis(25)
                )
            )
        );
    }

    #[test]
    fn config_rejects_invalid_values() {
        let invalid_addr = AppConfig::from_vars(|key| {
            (key == "HARPE_GRPC_ADDR").then(|| "not-an-address".to_owned())
        })
        .unwrap_err();
        assert!(matches!(invalid_addr, HarpeError::Validation(_)));

        let missing_model =
            AppConfig::from_vars(|key| (key == "HARPE_LLM_PROVIDER").then(|| "http".to_owned()))
                .unwrap_err();
        assert!(matches!(missing_model, HarpeError::Validation(_)));

        let unsupported_provider =
            AppConfig::from_vars(|key| (key == "HARPE_LLM_PROVIDER").then(|| "unknown".to_owned()))
                .unwrap_err();
        assert!(matches!(unsupported_provider, HarpeError::Validation(_)));

        let invalid_interval =
            AppConfig::from_vars(|key| (key == "HARPE_JOB_INTERVAL_MS").then(|| "fast".to_owned()))
                .unwrap_err();
        assert!(matches!(invalid_interval, HarpeError::Validation(_)));

        let invalid_batch =
            AppConfig::from_vars(|key| (key == "HARPE_JOB_BATCH_LIMIT").then(|| "many".to_owned()))
                .unwrap_err();
        assert!(matches!(invalid_batch, HarpeError::Validation(_)));

        let invalid_llm_timeout = AppConfig::from_vars(|key| match key {
            "HARPE_LLM_PROVIDER" => Some("http".to_owned()),
            "HARPE_LLM_BASE_URL" => Some("http://localhost".to_owned()),
            "HARPE_LLM_CHAT_MODEL" => Some("chat".to_owned()),
            "HARPE_LLM_EXTRACTION_MODEL" => Some("extract".to_owned()),
            "HARPE_LLM_EMBEDDING_MODEL" => Some("embed".to_owned()),
            "HARPE_LLM_TIMEOUT_MS" => Some("soon".to_owned()),
            _ => None,
        })
        .unwrap_err();
        assert!(matches!(invalid_llm_timeout, HarpeError::Validation(_)));

        let invalid_llm_retries = AppConfig::from_vars(|key| match key {
            "HARPE_LLM_PROVIDER" => Some("http".to_owned()),
            "HARPE_LLM_BASE_URL" => Some("http://localhost".to_owned()),
            "HARPE_LLM_CHAT_MODEL" => Some("chat".to_owned()),
            "HARPE_LLM_EXTRACTION_MODEL" => Some("extract".to_owned()),
            "HARPE_LLM_EMBEDDING_MODEL" => Some("embed".to_owned()),
            "HARPE_LLM_MAX_RETRIES" => Some("often".to_owned()),
            _ => None,
        })
        .unwrap_err();
        assert!(matches!(invalid_llm_retries, HarpeError::Validation(_)));
    }
}
