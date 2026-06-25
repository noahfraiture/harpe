#[cfg(test)]
mod tests {
    use chrono::Utc;

    use super::*;

    #[test]
    fn update_memory_payload_round_trips_json_value() {
        let payload = UpdateMemoryAfterTurnPayload::new(
            "game-1".to_owned(),
            "session-1".to_owned(),
            "message-1".to_owned(),
            "The gate opens.".to_owned(),
        );

        let decoded =
            UpdateMemoryAfterTurnPayload::from_value(payload.clone().into_value().unwrap())
                .unwrap();

        assert_eq!(decoded, payload);
    }

    #[test]
    fn update_memory_payload_rejects_malformed_json() {
        let err = UpdateMemoryAfterTurnPayload::from_value(serde_json::json!({
            "game_id": "game-1"
        }))
        .unwrap_err();

        assert!(matches!(err, HarpeError::Validation(_)));
    }

    #[test]
    fn extracted_entity_matching_ignores_case_and_whitespace() {
        assert!(same_name(" Mira ", "mira"));
        assert!(!same_name("Mira", "Kest"));

        let fact = WorldFact {
            id: "fact-1".to_owned(),
            game_id: "game-1".to_owned(),
            subject: "Silver Key".to_owned(),
            predicate: "Opens".to_owned(),
            object: "Lower Vault".to_owned(),
            content: "The silver key opens the lower vault.".to_owned(),
            confidence: 1.0,
            updated_at: chrono::Utc::now(),
        };

        assert!(same_fact(&fact, " silver key ", "opens", "lower vault"));
        assert!(!same_fact(&fact, "bronze key", "opens", "lower vault"));

        let character = Character {
            id: "character-1".to_owned(),
            game_id: "game-1".to_owned(),
            name: "Mira".to_owned(),
            description: "Gate scout".to_owned(),
            status: "alert".to_owned(),
            updated_at: chrono::Utc::now(),
        };
        let known_fact = WorldFact {
            id: "fact-2".to_owned(),
            subject: " mira ".to_owned(),
            ..fact.clone()
        };
        let unrelated_fact = WorldFact {
            id: "fact-3".to_owned(),
            subject: "Sea gate".to_owned(),
            object: "Harbor".to_owned(),
            ..fact
        };

        assert!(character_matches_fact(&character, &known_fact));
        assert!(!character_matches_fact(&character, &unrelated_fact));
    }

    #[test]
    fn retry_policy_uses_attempts_and_caps_delay() {
        let job = BackgroundJob {
            id: "job-1".to_owned(),
            kind: JobKind::UpdateMemoryAfterTurn,
            status: JobStatus::Running,
            payload: serde_json::json!({}),
            attempts: 2,
            max_attempts: 3,
            last_error: None,
            run_after: Utc::now(),
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };

        assert!(should_retry(&job));
        assert_eq!(retry_delay_for_attempt(0), Duration::from_secs(1));
        assert_eq!(retry_delay_for_attempt(2), Duration::from_secs(4));
        assert_eq!(retry_delay_for_attempt(99), Duration::from_secs(256));

        let exhausted = BackgroundJob { attempts: 3, ..job };
        assert!(!should_retry(&exhausted));
    }
}
