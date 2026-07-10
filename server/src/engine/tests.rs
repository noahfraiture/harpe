#[cfg(test)]
mod tests {
    use chrono::Utc;

    use super::*;
    use crate::domain::{MemoryChunk, MemoryHit};

    #[test]
    fn context_builder_adds_summary_memories_characters_and_recent_messages() {
        let now = Utc::now();
        let builder = ContextBuilder {
            recent_message_limit: 2,
            memory_limit: 1,
            character_limit: 1,
            budget: ContextBudget::default(),
            token_estimator: TokenEstimator::default(),
        };

        let chat = builder.build(ContextInputs {
            base_system_prompt: "You are the GM.".to_owned(),
            summary: Some(StorySummary {
                session_id: "session-1".to_owned(),
                content: "The party entered the archive.".to_owned(),
                updated_at: now,
            }),
            recent_events: vec![Event {
                id: "event-1".to_owned(),
                session_id: "session-1".to_owned(),
                summary: "Mira found the archive stairs.".to_owned(),
                importance: 3,
                created_at: now,
            }],
            memories: vec![MemoryHit {
                chunk: MemoryChunk {
                    id: "memory-1".to_owned(),
                    session_id: "session-1".to_owned(),
                    kind: "event".to_owned(),
                    content: "A silver key opens the lower vault.".to_owned(),
                    embedding: vec![1.0],
                    created_at: now,
                },
                score: 0.9,
            }],
            characters: vec![Character {
                id: "character-1".to_owned(),
                game_id: "game-1".to_owned(),
                name: "Mira".to_owned(),
                description: "Archivist".to_owned(),
                status: "wounded".to_owned(),
                updated_at: now,
            }],
            world_facts: vec![WorldFact {
                id: "fact-1".to_owned(),
                game_id: "game-1".to_owned(),
                subject: "silver key".to_owned(),
                predicate: "opens".to_owned(),
                object: "lower vault".to_owned(),
                content: "The silver key opens the lower vault.".to_owned(),
                confidence: 0.9,
                updated_at: now,
            }],
            locations: vec![Location {
                id: "location-1".to_owned(),
                game_id: "game-1".to_owned(),
                name: "Lower Vault".to_owned(),
                description: "A sealed chamber under the archive".to_owned(),
                updated_at: now,
            }],
            recent_messages: vec![
                Message {
                    id: "m1".to_owned(),
                    session_id: "session-1".to_owned(),
                    role: MessageRole::User,
                    content: "first".to_owned(),
                    created_at: now,
                },
                Message {
                    id: "m2".to_owned(),
                    session_id: "session-1".to_owned(),
                    role: MessageRole::Assistant,
                    content: "second".to_owned(),
                    created_at: now,
                },
                Message {
                    id: "m3".to_owned(),
                    session_id: "session-1".to_owned(),
                    role: MessageRole::User,
                    content: "third".to_owned(),
                    created_at: now,
                },
            ],
        });

        assert_eq!(chat.messages.len(), 3);
        assert!(chat.messages[0].content.contains("Story summary"));
        assert!(chat.messages[0].content.contains("Recent events"));
        assert!(chat.messages[0].content.contains("silver key"));
        assert!(chat.messages[0].content.contains("Mira"));
        assert!(chat.messages[0].content.contains("Lower Vault"));
        assert_eq!(chat.messages[1].content, "second");
        assert_eq!(chat.messages[2].content, "third");
    }

    #[test]
    fn context_builder_drops_low_priority_candidates_when_budget_is_tight() {
        let now = Utc::now();
        let builder = ContextBuilder {
            recent_message_limit: 0,
            memory_limit: 2,
            character_limit: 2,
            budget: ContextBudget {
                max_context_tokens: 64,
                reserved_response_tokens: 0,
            },
            token_estimator: TokenEstimator::default(),
        };

        let chat = builder.build(ContextInputs {
            base_system_prompt: "You are the GM.".to_owned(),
            summary: Some(StorySummary {
                session_id: "session-1".to_owned(),
                content: "The party entered the archive.".to_owned(),
                updated_at: now,
            }),
            memories: vec![MemoryHit {
                chunk: MemoryChunk {
                    id: "memory-1".to_owned(),
                    session_id: "session-1".to_owned(),
                    kind: "event".to_owned(),
                    content: "The key is hidden under the green altar.".to_owned(),
                    embedding: vec![1.0],
                    created_at: now,
                },
                score: 1.0,
            }],
            locations: vec![Location {
                id: "location-1".to_owned(),
                game_id: "game-1".to_owned(),
                name: "A very long location name that should not fit".to_owned(),
                description: "This location description is intentionally verbose so the low-priority location candidate is dropped before higher-value story context.".to_owned(),
                updated_at: now,
            }],
            ..ContextInputs::default()
        });

        let system = &chat.messages[0].content;
        assert!(system.contains("Story summary"));
        assert!(system.contains("green altar"));
        assert!(!system.contains("very long location"));
    }

    #[test]
    fn context_builder_preserves_recent_message_order_after_budget_selection() {
        let now = Utc::now();
        let builder = ContextBuilder {
            recent_message_limit: 3,
            memory_limit: 0,
            character_limit: 0,
            budget: ContextBudget {
                max_context_tokens: 32,
                reserved_response_tokens: 0,
            },
            token_estimator: TokenEstimator::default(),
        };

        let chat = builder.build(ContextInputs {
            base_system_prompt: "You are the GM.".to_owned(),
            recent_messages: vec![
                Message {
                    id: "m1".to_owned(),
                    session_id: "session-1".to_owned(),
                    role: MessageRole::User,
                    content: "old message with enough length to be dropped first because it takes too much of the recent message budget".to_owned(),
                    created_at: now,
                },
                Message {
                    id: "m2".to_owned(),
                    session_id: "session-1".to_owned(),
                    role: MessageRole::Assistant,
                    content: "middle".to_owned(),
                    created_at: now,
                },
                Message {
                    id: "m3".to_owned(),
                    session_id: "session-1".to_owned(),
                    role: MessageRole::User,
                    content: "newest".to_owned(),
                    created_at: now,
                },
            ],
            ..ContextInputs::default()
        });

        assert_eq!(chat.messages.len(), 3);
        assert_eq!(chat.messages[1].content, "middle");
        assert_eq!(chat.messages[2].content, "newest");
    }

    #[test]
    fn ranked_candidates_prioritize_summary_then_relevant_memory() {
        let now = Utc::now();
        let builder = ContextBuilder::default();
        let candidates = builder.ranked_candidates(&ContextInputs {
            summary: Some(StorySummary {
                session_id: "session-1".to_owned(),
                content: "The party entered the archive.".to_owned(),
                updated_at: now,
            }),
            recent_events: vec![Event {
                id: "event-1".to_owned(),
                session_id: "session-1".to_owned(),
                summary: "A minor bell rang.".to_owned(),
                importance: 1,
                created_at: now,
            }],
            memories: vec![MemoryHit {
                chunk: MemoryChunk {
                    id: "memory-1".to_owned(),
                    session_id: "session-1".to_owned(),
                    kind: "fact".to_owned(),
                    content: "The vault answer is ash.".to_owned(),
                    embedding: vec![],
                    created_at: now,
                },
                score: 1.0,
            }],
            ..ContextInputs::default()
        });

        assert_eq!(candidates[0].kind, ContextKind::StorySummary);
        assert_eq!(candidates[1].kind, ContextKind::Memory);
        assert_eq!(candidates[2].kind, ContextKind::Event);
    }

    #[test]
    fn estimate_tokens_uses_model_aware_conservative_estimate() {
        assert_eq!(estimate_tokens(""), 1);
        assert_eq!(estimate_tokens("abcd"), 1);
        assert_eq!(estimate_tokens("abcde"), 2);

        let openai = TokenEstimator::for_model("gpt-4o");
        let llama = TokenEstimator::for_model("llama-3-8b");
        let punctuated = "Mira: wait... now!";
        assert_eq!(openai.profile, TokenizerProfile::OpenAi);
        assert_eq!(llama.profile, TokenizerProfile::Llama);
        assert!(llama.estimate(punctuated) >= openai.estimate(punctuated));
    }

    #[test]
    fn context_builder_uses_model_context_window_presets() {
        let claude = ContextBuilder::for_model("claude-sonnet-200k");
        assert_eq!(claude.token_estimator.profile, TokenizerProfile::Anthropic);
        assert_eq!(claude.budget.max_context_tokens, 200_000);
        assert_eq!(claude.budget.reserved_response_tokens, 8_000);

        let local = ContextBuilder::for_model("llama-3-8k");
        assert_eq!(local.token_estimator.profile, TokenizerProfile::Llama);
        assert_eq!(local.budget.max_context_tokens, 8_000);
        assert_eq!(local.budget.reserved_response_tokens, 1_000);
    }

    #[test]
    fn context_budget_supports_explicit_windows_and_model_family_fallbacks() {
        let default = ContextBudget::default();
        assert_eq!(ContextBudget::for_model("   "), default);
        assert_eq!(ContextBudget::for_model("unknown-model"), default);

        for (model, expected_tokens) in [
            ("custom-1000k", 1_000_000),
            ("custom-1m", 1_000_000),
            ("custom-128k", 128_000),
            ("custom-64k", 64_000),
            ("custom-32k", 32_000),
            ("custom-16k", 16_000),
            ("custom-4k", 4_000),
            ("claude-opus", 200_000),
            ("gpt-4.1", 1_000_000),
            ("gpt-4o", 128_000),
            ("o3-mini", 128_000),
            ("o4-mini", 128_000),
            ("gpt-3.5", 32_000),
        ] {
            assert_eq!(
                ContextBudget::for_model(model).max_context_tokens,
                expected_tokens,
                "unexpected context window for {model}"
            );
        }

        assert_eq!(
            ContextBudget::for_model("custom-4k").reserved_response_tokens,
            1_000
        );
        assert_eq!(
            ContextBudget::for_model("custom-1m").reserved_response_tokens,
            8_000
        );
        assert_eq!(
            ContextBudget {
                max_context_tokens: 1_000,
                reserved_response_tokens: 2_000,
            }
            .max_prompt_tokens(),
            0
        );
    }

    #[test]
    fn tokenizer_profiles_cover_all_supported_model_families() {
        assert_eq!(
            TokenizerProfile::for_model("claude-opus"),
            TokenizerProfile::Anthropic
        );
        assert_eq!(
            TokenizerProfile::for_model("mixtral-8x7b"),
            TokenizerProfile::Mistral
        );
        assert_eq!(
            TokenizerProfile::for_model("o3-mini"),
            TokenizerProfile::OpenAi
        );
        assert_eq!(
            TokenizerProfile::for_model("unknown-model"),
            TokenizerProfile::Generic
        );

        let content = "Mira says: ouvre la porte!";
        assert!(
            TokenEstimator {
                profile: TokenizerProfile::Mistral,
            }
            .estimate(content)
                >= TokenEstimator {
                    profile: TokenizerProfile::Anthropic,
                }
                .estimate(content)
        );
    }

    #[test]
    fn trusted_prompt_and_entity_formatters_keep_state_readable() {
        let now = Utc::now();

        assert!(trusted_system_prompt("").starts_with("Trusted game state follows."));
        assert!(trusted_system_prompt("Run a mystery.").starts_with("Run a mystery."));

        assert_eq!(
            format_world_fact(&WorldFact {
                id: "fact-1".to_owned(),
                game_id: "game-1".to_owned(),
                subject: " silver key ".to_owned(),
                predicate: " opens ".to_owned(),
                object: " lower vault ".to_owned(),
                content: " ".to_owned(),
                confidence: 0.9,
                updated_at: now,
            }),
            "- silver key opens lower vault"
        );
        assert_eq!(
            format_location(&Location {
                id: "location-1".to_owned(),
                game_id: "game-1".to_owned(),
                name: " Lower Vault ".to_owned(),
                description: " ".to_owned(),
                updated_at: now,
            }),
            "- Lower Vault"
        );
    }

    #[test]
    fn cosine_similarity_handles_mismatch_and_zero_vectors() {
        assert_eq!(cosine_similarity(&[1.0], &[1.0, 0.0]), 0.0);
        assert_eq!(cosine_similarity(&[0.0, 0.0], &[1.0, 0.0]), 0.0);
        assert!((cosine_similarity(&[1.0, 0.0], &[1.0, 0.0]) - 1.0).abs() < 0.001);
    }
}
