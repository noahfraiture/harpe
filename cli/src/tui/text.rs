use harpe_proto::pb;

pub(super) fn role_name(role: i32) -> &'static str {
    match pb::MessageRole::try_from(role).ok() {
        Some(pb::MessageRole::System) => "system",
        Some(pb::MessageRole::User) => "you",
        Some(pb::MessageRole::Assistant) => "narrator",
        Some(pb::MessageRole::Unspecified) | None => "unknown",
    }
}

pub(super) fn blank_as<'a>(value: &'a str, fallback: &'a str) -> &'a str {
    if value.trim().is_empty() {
        fallback
    } else {
        value
    }
}

pub(super) fn first_sentences(content: &str, count: usize) -> String {
    let mut sentences = Vec::new();
    let mut start = 0;
    for (index, char) in content.char_indices() {
        if matches!(char, '.' | '!' | '?') {
            let end = index + char.len_utf8();
            let sentence = content[start..end].trim();
            if !sentence.is_empty() {
                sentences.push(sentence.to_owned());
            }
            start = end;
            if sentences.len() >= count {
                break;
            }
        }
    }
    if sentences.is_empty() {
        truncate(content.trim(), 220)
    } else {
        sentences.join(" ")
    }
}

pub(super) fn truncate(content: &str, limit: usize) -> String {
    let trimmed = content.trim();
    if trimmed.chars().count() <= limit {
        return trimmed.to_owned();
    }
    let mut output = trimmed
        .chars()
        .take(limit.saturating_sub(3))
        .collect::<String>();
    output.push_str("...");
    output
}

pub(super) fn wrap_owned(content: &str, width: usize) -> Vec<String> {
    let width = width.max(12);
    textwrap::wrap(content, width)
        .into_iter()
        .map(|line| line.into_owned())
        .collect()
}
