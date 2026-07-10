use std::io::Write;

use harpe_proto::pb::{
    GetCharacterRequest, GetStorySummaryRequest, ListCharactersRequest, ListEventsRequest,
    ListLocationsRequest, ListWorldFactsRequest, SearchMemoryRequest,
    memory_service_client::MemoryServiceClient,
};
use serde_json::json;
use tonic::transport::Channel;

use crate::config::required_config_value;
use crate::output::{
    character_json, event_json, location_json, memory_hit_json, page_json, story_summary_json,
    world_fact_json, write_json,
};
use crate::{CliResult, ClientConfig, MemoryArgs, MemoryCommand, join_words, with_user};

pub(crate) async fn memory<W: Write>(
    channel: Channel,
    args: MemoryArgs,
    user_id: String,
    config: &ClientConfig,
    as_json: bool,
    writer: &mut W,
) -> CliResult<()> {
    let mut client = MemoryServiceClient::new(channel);
    match args.command {
        MemoryCommand::Summary { session_id } => {
            let response = client
                .get_story_summary(with_user(GetStorySummaryRequest { session_id }, &user_id)?)
                .await?
                .into_inner();
            if as_json {
                write_json(writer, &story_summary_json(&response))
            } else {
                writeln!(writer, "updated_at={}", response.updated_at)?;
                writeln!(writer, "{}", response.content)?;
                Ok(())
            }
        }
        MemoryCommand::Characters { game, page } => {
            let game =
                required_config_value("game id", game.as_deref().or(config.game_id.as_deref()))?;
            let response = client
                .list_characters(with_user(
                    ListCharactersRequest {
                        game_id: game,
                        limit: 0,
                        page: Some(page.request()),
                    },
                    &user_id,
                )?)
                .await?
                .into_inner();
            if as_json {
                write_json(
                    writer,
                    &json!({
                        "characters": response.characters.iter().map(character_json).collect::<Vec<_>>(),
                        "page": page_json(response.page.as_ref()),
                    }),
                )
            } else {
                for character in response.characters {
                    writeln!(
                        writer,
                        "{}\t{}\tstatus={}\t{}",
                        character.id, character.name, character.status, character.description
                    )?;
                }
                Ok(())
            }
        }
        MemoryCommand::Character { character_id } => {
            let response = client
                .get_character(with_user(GetCharacterRequest { character_id }, &user_id)?)
                .await?
                .into_inner();
            if as_json {
                write_json(writer, &character_json(&response))
            } else {
                writeln!(
                    writer,
                    "{}\t{}\tstatus={}\n{}",
                    response.id, response.name, response.status, response.description
                )?;
                Ok(())
            }
        }
        MemoryCommand::Events { session_id, page } => {
            let response = client
                .list_events(with_user(
                    ListEventsRequest {
                        session_id,
                        limit: 0,
                        page: Some(page.request()),
                    },
                    &user_id,
                )?)
                .await?
                .into_inner();
            if as_json {
                write_json(
                    writer,
                    &json!({
                        "events": response.events.iter().map(event_json).collect::<Vec<_>>(),
                        "page": page_json(response.page.as_ref()),
                    }),
                )
            } else {
                for event in response.events {
                    writeln!(
                        writer,
                        "{}\timportance={}\t{}\t{}",
                        event.id, event.importance, event.created_at, event.summary
                    )?;
                }
                Ok(())
            }
        }
        MemoryCommand::Facts { game, page } => {
            let game =
                required_config_value("game id", game.as_deref().or(config.game_id.as_deref()))?;
            let response = client
                .list_world_facts(with_user(
                    ListWorldFactsRequest {
                        game_id: game,
                        limit: 0,
                        page: Some(page.request()),
                    },
                    &user_id,
                )?)
                .await?
                .into_inner();
            if as_json {
                write_json(
                    writer,
                    &json!({
                        "facts": response.facts.iter().map(world_fact_json).collect::<Vec<_>>(),
                        "page": page_json(response.page.as_ref()),
                    }),
                )
            } else {
                for fact in response.facts {
                    writeln!(
                        writer,
                        "{}\tconfidence={:.2}\t{}",
                        fact.id, fact.confidence, fact.content
                    )?;
                }
                Ok(())
            }
        }
        MemoryCommand::Locations { game, page } => {
            let game =
                required_config_value("game id", game.as_deref().or(config.game_id.as_deref()))?;
            let response = client
                .list_locations(with_user(
                    ListLocationsRequest {
                        game_id: game,
                        limit: 0,
                        page: Some(page.request()),
                    },
                    &user_id,
                )?)
                .await?
                .into_inner();
            if as_json {
                write_json(
                    writer,
                    &json!({
                        "locations": response.locations.iter().map(location_json).collect::<Vec<_>>(),
                        "page": page_json(response.page.as_ref()),
                    }),
                )
            } else {
                for location in response.locations {
                    writeln!(
                        writer,
                        "{}\t{}\t{}",
                        location.id, location.name, location.description
                    )?;
                }
                Ok(())
            }
        }
        MemoryCommand::Search {
            session_id,
            query,
            page,
        } => {
            let response = client
                .search_memory(with_user(
                    SearchMemoryRequest {
                        session_id,
                        query: join_words(query),
                        limit: 0,
                        page: Some(page.request()),
                    },
                    &user_id,
                )?)
                .await?
                .into_inner();
            if as_json {
                write_json(
                    writer,
                    &json!({
                        "hits": response.hits.iter().map(memory_hit_json).collect::<Vec<_>>(),
                        "page": page_json(response.page.as_ref()),
                    }),
                )
            } else {
                for hit in response.hits {
                    writeln!(
                        writer,
                        "{}\tscore={:.4}\tkind={}\t{}",
                        hit.id, hit.score, hit.kind, hit.content
                    )?;
                }
                Ok(())
            }
        }
    }
}
