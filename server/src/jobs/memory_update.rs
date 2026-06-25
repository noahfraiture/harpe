use crate::Result;
use crate::domain::{
    Character, Event, GraphRelationKind, Location, MemoryExtraction, MemoryHit, NewEvent,
    NewMemoryChunk, Session, UpsertCharacter, UpsertLocation, UpsertStorySummary, UpsertWorldFact,
    WorldFact,
};
use crate::llm::{ExtractMemoryRequest, LlmClient, SummarizeRequest};
use crate::store::HarpeStore;

#[tracing::instrument(skip_all, fields(session_id = %session.id, game_id = %game_id))]
pub async fn update_memory_after_turn(
    session: &Session,
    game_id: &str,
    assistant_content: &str,
    store: &dyn HarpeStore,
    llm: &dyn LlmClient,
) -> Result<()> {
    let previous_summary = store
        .get_story_summary(&session.id)
        .await?
        .map(|summary| summary.content);
    let recent_messages = store.list_recent_messages(&session.id, 24).await?;
    let updated_summary = llm
        .summarize(SummarizeRequest {
            previous_summary,
            messages: recent_messages.clone(),
        })
        .await?;

    store
        .upsert_story_summary(UpsertStorySummary {
            session_id: session.id.clone(),
            content: updated_summary,
        })
        .await?;

    let embedding = llm.embed(assistant_content).await?;
    let turn_memory = store
        .save_memory_chunk(NewMemoryChunk {
            session_id: session.id.clone(),
            kind: "turn".to_owned(),
            content: assistant_content.to_owned(),
            embedding,
        })
        .await?;

    let extraction = llm
        .extract_memory(ExtractMemoryRequest {
            game_id: game_id.to_owned(),
            session_id: session.id.clone(),
            messages: recent_messages,
        })
        .await?;
    persist_extraction(
        session,
        game_id,
        extraction,
        Some(turn_memory.chunk.id),
        store,
        llm,
    )
    .await
}

#[tracing::instrument(
    skip_all,
    fields(
        session_id = %session.id,
        game_id = %game_id,
        event_count = extraction.events.len(),
        character_count = extraction.character_updates.len(),
        world_fact_count = extraction.world_facts.len(),
        location_count = extraction.locations.len(),
    )
)]
async fn persist_extraction(
    session: &Session,
    game_id: &str,
    extraction: MemoryExtraction,
    turn_memory_id: Option<String>,
    store: &dyn HarpeStore,
    llm: &dyn LlmClient,
) -> Result<()> {
    let mut saved_events = Vec::new();
    let mut saved_characters = Vec::new();
    let mut saved_facts = Vec::new();
    let mut saved_locations = Vec::new();
    let mut fact_memory_edges = Vec::<(String, String)>::new();

    for event in extraction.events {
        if event.summary.trim().is_empty() {
            continue;
        }

        let event = store
            .save_event(NewEvent {
                session_id: session.id.clone(),
                summary: event.summary,
                importance: event.importance,
            })
            .await?;
        let _ = save_embedded_memory(session, "event", event.summary.as_str(), store, llm).await?;
        saved_events.push(event);
    }

    let existing_characters = store.list_characters(game_id).await?;
    for character in extraction.character_updates {
        if character.name.trim().is_empty() {
            continue;
        }

        let existing_id = existing_characters
            .iter()
            .find(|existing| same_name(&existing.name, &character.name))
            .map(|existing| existing.id.clone());
        let character = store
            .upsert_character(UpsertCharacter {
                id: existing_id,
                game_id: game_id.to_owned(),
                name: character.name,
                description: character.description,
                status: character.status,
            })
            .await?;
        let _ = save_embedded_memory(
            session,
            "character",
            &format!(
                "{} | status: {} | {}",
                character.name, character.status, character.description
            ),
            store,
            llm,
        )
        .await?;
        saved_characters.push(character);
    }

    let existing_facts = store.list_world_facts(game_id, 100).await?;
    for fact in extraction.world_facts {
        if fact.subject.trim().is_empty()
            || fact.predicate.trim().is_empty()
            || fact.object.trim().is_empty()
        {
            continue;
        }

        let existing_id = existing_facts
            .iter()
            .find(|existing| same_fact(existing, &fact.subject, &fact.predicate, &fact.object))
            .map(|existing| existing.id.clone());
        let fact = store
            .upsert_world_fact(UpsertWorldFact {
                id: existing_id,
                game_id: game_id.to_owned(),
                subject: fact.subject,
                predicate: fact.predicate,
                object: fact.object,
                content: fact.content,
                confidence: fact.confidence,
            })
            .await?;
        if let Some(turn_memory_id) = turn_memory_id.as_deref() {
            fact_memory_edges.push((turn_memory_id.to_owned(), fact.id.clone()));
        }
        if let Some(memory) =
            save_embedded_memory(session, "world_fact", &fact.content, store, llm).await?
        {
            fact_memory_edges.push((memory.chunk.id, fact.id.clone()));
        }
        saved_facts.push(fact);
    }

    let existing_locations = store.list_locations(game_id).await?;
    for location in extraction.locations {
        if location.name.trim().is_empty() {
            continue;
        }

        let existing_id = existing_locations
            .iter()
            .find(|existing| same_name(&existing.name, &location.name))
            .map(|existing| existing.id.clone());
        let location = store
            .upsert_location(UpsertLocation {
                id: existing_id,
                game_id: game_id.to_owned(),
                name: location.name,
                description: location.description,
            })
            .await?;
        let _ = save_embedded_memory(
            session,
            "location",
            &format!("{} | {}", location.name, location.description),
            store,
            llm,
        )
        .await?;
        saved_locations.push(location);
    }

    link_extraction_graph(
        store,
        &saved_events,
        &saved_characters,
        &saved_facts,
        &saved_locations,
        &fact_memory_edges,
    )
    .await?;

    Ok(())
}

async fn save_embedded_memory(
    session: &Session,
    kind: &str,
    content: &str,
    store: &dyn HarpeStore,
    llm: &dyn LlmClient,
) -> Result<Option<MemoryHit>> {
    let content = content.trim();
    if content.is_empty() {
        return Ok(None);
    }

    let embedding = llm.embed(content).await?;
    let hit = store
        .save_memory_chunk(NewMemoryChunk {
            session_id: session.id.clone(),
            kind: kind.to_owned(),
            content: content.to_owned(),
            embedding,
        })
        .await?;

    Ok(Some(hit))
}

async fn link_extraction_graph(
    store: &dyn HarpeStore,
    events: &[Event],
    characters: &[Character],
    facts: &[WorldFact],
    locations: &[Location],
    fact_memory_edges: &[(String, String)],
) -> Result<()> {
    for event in events {
        for character in characters {
            store
                .upsert_graph_edge(
                    GraphRelationKind::EventInvolvesCharacter,
                    &event.id,
                    &character.id,
                )
                .await?;
        }
        for location in locations {
            store
                .upsert_graph_edge(
                    GraphRelationKind::EventHappenedAtLocation,
                    &event.id,
                    &location.id,
                )
                .await?;
        }
    }

    for character in characters {
        for fact in facts
            .iter()
            .filter(|fact| character_matches_fact(character, fact))
        {
            store
                .upsert_graph_edge(
                    GraphRelationKind::CharacterKnowsWorldFact,
                    &character.id,
                    &fact.id,
                )
                .await?;
        }
    }

    for (memory_id, fact_id) in fact_memory_edges {
        store
            .upsert_graph_edge(
                GraphRelationKind::MemorySupportsWorldFact,
                memory_id,
                fact_id,
            )
            .await?;
    }

    Ok(())
}

pub(super) fn same_name(left: &str, right: &str) -> bool {
    left.trim().eq_ignore_ascii_case(right.trim())
}

pub(super) fn character_matches_fact(character: &Character, fact: &WorldFact) -> bool {
    same_name(&character.name, &fact.subject) || same_name(&character.name, &fact.object)
}

pub(super) fn same_fact(fact: &WorldFact, subject: &str, predicate: &str, object: &str) -> bool {
    same_name(&fact.subject, subject)
        && same_name(&fact.predicate, predicate)
        && same_name(&fact.object, object)
}
