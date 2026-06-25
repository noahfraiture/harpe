#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TokenizerProfile {
    #[default]
    Generic,
    OpenAi,
    Anthropic,
    Llama,
    Mistral,
}

impl TokenizerProfile {
    pub fn for_model(model_name: &str) -> Self {
        let model = model_name.trim().to_ascii_lowercase();

        if model.contains("claude") {
            Self::Anthropic
        } else if model.contains("llama") {
            Self::Llama
        } else if model.contains("mistral") || model.contains("mixtral") {
            Self::Mistral
        } else if model.contains("gpt") || model.starts_with('o') {
            Self::OpenAi
        } else {
            Self::Generic
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TokenEstimator {
    pub profile: TokenizerProfile,
}

impl TokenEstimator {
    pub fn for_model(model_name: &str) -> Self {
        Self {
            profile: TokenizerProfile::for_model(model_name),
        }
    }

    pub fn estimate(&self, content: &str) -> usize {
        estimate_tokens_with_profile(content, self.profile)
    }
}

pub fn estimate_tokens(content: &str) -> usize {
    TokenEstimator::default().estimate(content)
}

fn estimate_tokens_with_profile(content: &str, profile: TokenizerProfile) -> usize {
    let chars = content.chars().count();
    let words = content.split_whitespace().count();
    let punctuation = content
        .chars()
        .filter(|char| char.is_ascii_punctuation())
        .count();
    let non_ascii = content
        .chars()
        .filter(|char| !char.is_ascii() && !char.is_whitespace())
        .count();
    let chars_per_token_x10 = match profile {
        TokenizerProfile::Generic | TokenizerProfile::OpenAi => 40,
        TokenizerProfile::Anthropic => 38,
        TokenizerProfile::Llama => 32,
        TokenizerProfile::Mistral => 34,
    };
    let punctuation_divisor = match profile {
        TokenizerProfile::Generic => 3,
        TokenizerProfile::OpenAi | TokenizerProfile::Anthropic => 2,
        TokenizerProfile::Llama | TokenizerProfile::Mistral => 1,
    };
    let char_estimate = (chars * 10).div_ceil(chars_per_token_x10);
    let lexical_estimate = words
        .saturating_add(punctuation.div_ceil(punctuation_divisor))
        .saturating_add(non_ascii);

    char_estimate.max(lexical_estimate).max(1)
}
