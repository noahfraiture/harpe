use crate::domain::{
    Character, Event, Location, MemoryHit, Message, MessageRole, StorySummary, WorldFact,
};
use crate::llm::{ChatMessage, ChatRequest};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContextBuilder {
    pub recent_message_limit: usize,
    pub memory_limit: usize,
    pub character_limit: usize,
}

impl Default for ContextBuilder {
    fn default() -> Self {
        Self {
            recent_message_limit: 24,
            memory_limit: 8,
            character_limit: 12,
        }
    }
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

impl ContextBuilder {
    pub fn build(&self, input: ContextInputs) -> ChatRequest {
        let mut messages = vec![ChatMessage {
            role: MessageRole::System,
            content: self.system_context(&input),
        }];

        messages.extend(
            input
                .recent_messages
                .into_iter()
                .rev()
                .take(self.recent_message_limit)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .map(|message| ChatMessage {
                    role: message.role,
                    content: message.content,
                }),
        );

        ChatRequest { messages }
    }

    fn system_context(&self, input: &ContextInputs) -> String {
        let mut sections = vec![input.base_system_prompt.trim().to_owned()];

        if let Some(summary) = &input.summary
            && !summary.content.trim().is_empty()
        {
            sections.push(format!("Story summary:\n{}", summary.content.trim()));
        }

        let recent_events = input
            .recent_events
            .iter()
            .filter(|event| !event.summary.trim().is_empty())
            .map(|event| format!("- {}", event.summary.trim()))
            .collect::<Vec<_>>();

        if !recent_events.is_empty() {
            sections.push(format!("Recent events:\n{}", recent_events.join("\n")));
        }

        let memories = input
            .memories
            .iter()
            .take(self.memory_limit)
            .filter(|hit| !hit.chunk.content.trim().is_empty())
            .map(|hit| format!("- [{}] {}", hit.chunk.kind, hit.chunk.content.trim()))
            .collect::<Vec<_>>();

        if !memories.is_empty() {
            sections.push(format!("Relevant memories:\n{}", memories.join("\n")));
        }

        let characters = input
            .characters
            .iter()
            .take(self.character_limit)
            .map(format_character)
            .collect::<Vec<_>>();

        if !characters.is_empty() {
            sections.push(format!("Known characters:\n{}", characters.join("\n")));
        }

        let world_facts = input
            .world_facts
            .iter()
            .filter(|fact| !fact.content.trim().is_empty())
            .map(format_world_fact)
            .collect::<Vec<_>>();

        if !world_facts.is_empty() {
            sections.push(format!("World facts:\n{}", world_facts.join("\n")));
        }

        let locations = input
            .locations
            .iter()
            .filter(|location| !location.name.trim().is_empty())
            .map(format_location)
            .collect::<Vec<_>>();

        if !locations.is_empty() {
            sections.push(format!("Known locations:\n{}", locations.join("\n")));
        }

        sections
            .into_iter()
            .filter(|section| !section.trim().is_empty())
            .collect::<Vec<_>>()
            .join("\n\n")
    }
}

fn format_character(character: &Character) -> String {
    let mut parts = vec![character.name.clone()];

    if !character.status.trim().is_empty() {
        parts.push(format!("status: {}", character.status.trim()));
    }

    if !character.description.trim().is_empty() {
        parts.push(character.description.trim().to_owned());
    }

    format!("- {}", parts.join(" | "))
}

fn format_world_fact(fact: &WorldFact) -> String {
    if fact.content.trim().is_empty() {
        format!(
            "- {} {} {}",
            fact.subject.trim(),
            fact.predicate.trim(),
            fact.object.trim()
        )
    } else {
        format!("- {}", fact.content.trim())
    }
}

fn format_location(location: &Location) -> String {
    if location.description.trim().is_empty() {
        format!("- {}", location.name.trim())
    } else {
        format!(
            "- {} | {}",
            location.name.trim(),
            location.description.trim()
        )
    }
}

pub fn cosine_similarity(left: &[f32], right: &[f32]) -> f32 {
    if left.is_empty() || right.is_empty() || left.len() != right.len() {
        return 0.0;
    }

    let mut dot = 0.0;
    let mut left_norm = 0.0;
    let mut right_norm = 0.0;

    for (left, right) in left.iter().zip(right.iter()) {
        dot += left * right;
        left_norm += left * left;
        right_norm += right * right;
    }

    if left_norm == 0.0 || right_norm == 0.0 {
        return 0.0;
    }

    dot / (left_norm.sqrt() * right_norm.sqrt())
}

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
    fn cosine_similarity_handles_mismatch_and_zero_vectors() {
        assert_eq!(cosine_similarity(&[1.0], &[1.0, 0.0]), 0.0);
        assert_eq!(cosine_similarity(&[0.0, 0.0], &[1.0, 0.0]), 0.0);
        assert!((cosine_similarity(&[1.0, 0.0], &[1.0, 0.0]) - 1.0).abs() < 0.001);
    }
}
