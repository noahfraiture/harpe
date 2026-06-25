use crate::domain::{ExtractedEvent, MemoryExtraction, Message, MessageRole};
use crate::{HarpeError, Result};

pub(super) fn extract_fallback_event(messages: Vec<Message>) -> MemoryExtraction {
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

pub(super) fn parse_memory_extraction(content: &str) -> Result<MemoryExtraction> {
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

    if let Some(embedded) = embedded_json_object(trimmed) {
        return serde_json::from_str(embedded)
            .map_err(|error| HarpeError::Llm(format!("invalid memory extraction JSON: {error}")));
    }

    Err(HarpeError::Llm("invalid memory extraction JSON".to_owned()))
}

pub(super) fn strip_code_fence(content: &str) -> Option<&str> {
    if !content.starts_with("```") {
        return None;
    }

    let open_end = content.find('\n')?;
    let body = &content[open_end + 1..];
    let close_start = body.rfind("```")?;

    Some(body[..close_start].trim())
}

fn embedded_json_object(content: &str) -> Option<&str> {
    let start = content.find('{')?;
    let end = content.rfind('}')?;

    (start < end).then_some(&content[start..=end])
}
