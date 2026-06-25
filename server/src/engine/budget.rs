#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContextBudget {
    pub max_context_tokens: usize,
    pub reserved_response_tokens: usize,
}

impl ContextBudget {
    pub fn max_prompt_tokens(&self) -> usize {
        self.max_context_tokens
            .saturating_sub(self.reserved_response_tokens)
    }

    pub fn for_model(model_name: &str) -> Self {
        let mut budget = Self::default();
        if let Some(max_context_tokens) = context_window_from_model_name(model_name) {
            budget.max_context_tokens = max_context_tokens;
            budget.reserved_response_tokens = response_reserve_for_context(max_context_tokens);
        }

        budget
    }
}

impl Default for ContextBudget {
    fn default() -> Self {
        Self {
            max_context_tokens: 32_000,
            reserved_response_tokens: 2_000,
        }
    }
}

fn context_window_from_model_name(model_name: &str) -> Option<usize> {
    let model = model_name.trim().to_ascii_lowercase();
    if model.is_empty() {
        return None;
    }

    [
        ("1000k", 1_000_000),
        ("1m", 1_000_000),
        ("200k", 200_000),
        ("128k", 128_000),
        ("64k", 64_000),
        ("32k", 32_000),
        ("16k", 16_000),
        ("8k", 8_000),
        ("4k", 4_000),
    ]
    .into_iter()
    .find_map(|(needle, tokens)| model.contains(needle).then_some(tokens))
    .or_else(|| {
        if model.contains("claude") {
            Some(200_000)
        } else if model.contains("gpt-4.1") {
            Some(1_000_000)
        } else if model.contains("gpt-4o") || model.starts_with("o3") || model.starts_with("o4") {
            Some(128_000)
        } else if model.contains("gpt") {
            Some(32_000)
        } else {
            None
        }
    })
}

fn response_reserve_for_context(max_context_tokens: usize) -> usize {
    (max_context_tokens / 16).clamp(1_000, 8_000)
}
