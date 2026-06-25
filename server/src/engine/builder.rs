use crate::domain::{
    Character, Event, Location, MemoryHit, Message, MessageRole, StorySummary, WorldFact,
};
use crate::llm::{ChatMessage, ChatRequest};

use super::budget::ContextBudget;
use super::format::{
    format_character, format_location, format_world_fact, render_sections, trusted_system_prompt,
};
use super::token::TokenEstimator;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContextBuilder {
    pub recent_message_limit: usize,
    pub memory_limit: usize,
    pub character_limit: usize,
    pub budget: ContextBudget,
    pub token_estimator: TokenEstimator,
}

impl Default for ContextBuilder {
    fn default() -> Self {
        Self {
            recent_message_limit: 24,
            memory_limit: 8,
            character_limit: 12,
            budget: ContextBudget::default(),
            token_estimator: TokenEstimator::default(),
        }
    }
}

impl ContextBuilder {
    pub fn for_model(model_name: &str) -> Self {
        Self {
            budget: ContextBudget::for_model(model_name),
            token_estimator: TokenEstimator::for_model(model_name),
            ..Self::default()
        }
    }

    pub fn with_budget(mut self, budget: ContextBudget) -> Self {
        self.budget = budget;
        self
    }

    pub fn with_token_estimator(mut self, token_estimator: TokenEstimator) -> Self {
        self.token_estimator = token_estimator;
        self
    }

    pub fn build(&self, input: ContextInputs) -> ChatRequest {
        let prompt_budget = self.budget.max_prompt_tokens();
        let selected_messages =
            self.select_recent_messages(&input.recent_messages, prompt_budget / 2);
        let message_tokens = selected_messages
            .iter()
            .map(|message| self.estimate_tokens(&message.content))
            .sum::<usize>();
        let system_budget = prompt_budget.saturating_sub(message_tokens);
        let mut messages = vec![ChatMessage {
            role: MessageRole::System,
            content: self.system_context(&input, system_budget),
        }];

        messages.extend(selected_messages.into_iter().map(|message| ChatMessage {
            role: message.role,
            content: message.content,
        }));

        ChatRequest {
            messages,
            model: None,
        }
    }

    fn system_context(&self, input: &ContextInputs, token_budget: usize) -> String {
        let base_prompt = trusted_system_prompt(&input.base_system_prompt);
        let base_tokens = self.estimate_tokens(&base_prompt);
        let mut remaining_budget = token_budget.saturating_sub(base_tokens);
        let mut selected = Vec::new();

        for candidate in self.ranked_candidates(input) {
            if candidate.estimated_tokens <= remaining_budget {
                remaining_budget -= candidate.estimated_tokens;
                selected.push(candidate);
            }
        }

        let mut sections = vec![base_prompt];
        sections.extend(render_sections(selected));

        sections
            .into_iter()
            .filter(|section| !section.trim().is_empty())
            .collect::<Vec<_>>()
            .join("\n\n")
    }

    pub(crate) fn ranked_candidates(&self, input: &ContextInputs) -> Vec<ContextCandidate> {
        let mut candidates = Vec::new();
        if let Some(summary) = &input.summary
            && !summary.content.trim().is_empty()
        {
            candidates.push(candidate(
                ContextKind::StorySummary,
                summary.content.trim(),
                900,
                candidates.len(),
                self.token_estimator,
            ));
        }

        for event in input
            .recent_events
            .iter()
            .filter(|event| !event.summary.trim().is_empty())
        {
            candidates.push(candidate(
                ContextKind::Event,
                &format!("- {}", event.summary.trim()),
                600 + event.importance.clamp(1, 5) * 10,
                candidates.len(),
                self.token_estimator,
            ));
        }

        for hit in input
            .memories
            .iter()
            .take(self.memory_limit)
            .filter(|hit| !hit.chunk.content.trim().is_empty())
        {
            candidates.push(candidate(
                ContextKind::Memory,
                &format!("- [{}] {}", hit.chunk.kind, hit.chunk.content.trim()),
                700 + (hit.score.clamp(0.0, 1.0) * 100.0).round() as i32,
                candidates.len(),
                self.token_estimator,
            ));
        }

        for character in input
            .characters
            .iter()
            .take(self.character_limit)
            .filter(|character| !character.name.trim().is_empty())
        {
            candidates.push(candidate(
                ContextKind::Character,
                &format_character(character),
                650,
                candidates.len(),
                self.token_estimator,
            ));
        }

        for fact in input
            .world_facts
            .iter()
            .filter(|fact| !fact.content.trim().is_empty())
        {
            candidates.push(candidate(
                ContextKind::WorldFact,
                &format_world_fact(fact),
                550 + (fact.confidence.clamp(0.0, 1.0) * 100.0).round() as i32,
                candidates.len(),
                self.token_estimator,
            ));
        }

        for location in input
            .locations
            .iter()
            .filter(|location| !location.name.trim().is_empty())
        {
            candidates.push(candidate(
                ContextKind::Location,
                &format_location(location),
                400,
                candidates.len(),
                self.token_estimator,
            ));
        }

        candidates.sort_by(|left, right| {
            right
                .priority
                .cmp(&left.priority)
                .then_with(|| left.estimated_tokens.cmp(&right.estimated_tokens))
                .then_with(|| left.insertion_index.cmp(&right.insertion_index))
        });

        candidates
    }

    fn select_recent_messages(&self, messages: &[Message], token_budget: usize) -> Vec<Message> {
        let mut selected = Vec::new();
        let mut remaining_budget = token_budget;

        for message in messages.iter().rev().take(self.recent_message_limit) {
            let estimated_tokens = self.estimate_tokens(&message.content);
            if estimated_tokens <= remaining_budget {
                remaining_budget -= estimated_tokens;
                selected.push(message.clone());
            }
        }

        selected.reverse();
        selected
    }

    pub fn estimate_tokens(&self, content: &str) -> usize {
        self.token_estimator.estimate(content)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContextKind {
    StorySummary,
    Memory,
    Character,
    Event,
    WorldFact,
    Location,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContextCandidate {
    pub kind: ContextKind,
    pub content: String,
    pub priority: i32,
    pub estimated_tokens: usize,
    pub(super) insertion_index: usize,
}

#[derive(Clone, Debug, Default)]
pub struct ContextInputs {
    pub base_system_prompt: String,
    pub summary: Option<StorySummary>,
    pub recent_events: Vec<Event>,
    pub memories: Vec<MemoryHit>,
    pub characters: Vec<Character>,
    pub world_facts: Vec<WorldFact>,
    pub locations: Vec<Location>,
    pub recent_messages: Vec<Message>,
}

fn candidate(
    kind: ContextKind,
    content: &str,
    priority: i32,
    insertion_index: usize,
    token_estimator: TokenEstimator,
) -> ContextCandidate {
    let content = content.trim().to_owned();

    ContextCandidate {
        kind,
        estimated_tokens: token_estimator.estimate(&content) + 4,
        content,
        priority,
        insertion_index,
    }
}
