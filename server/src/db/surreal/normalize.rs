pub(super) fn normalize_limit(limit: usize) -> usize {
    match limit {
        0 => 50,
        1..=1_000 => limit,
        _ => 1_000,
    }
}

pub(super) fn memory_candidate_limit(limit: usize) -> usize {
    normalize_limit(limit).saturating_mul(8).clamp(32, 512)
}

pub(super) fn normalize_importance(importance: i32) -> i32 {
    importance.clamp(1, 5)
}

pub(super) fn normalize_max_attempts(max_attempts: i32) -> i32 {
    max_attempts.clamp(1, 10)
}

pub(super) fn normalize_confidence(confidence: f32) -> f32 {
    confidence.clamp(0.0, 1.0)
}

pub(super) fn world_fact_content(
    subject: &str,
    predicate: &str,
    object: &str,
    content: &str,
) -> String {
    if content.trim().is_empty() {
        format!("{} {} {}", subject.trim(), predicate.trim(), object.trim())
    } else {
        content.to_owned()
    }
}
