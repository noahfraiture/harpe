use super::*;

pub(crate) async fn play<R: BufRead, W: Write>(
    channel: Channel,
    args: PlayArgs,
    user_id: String,
    config: &ClientConfig,
    as_json: bool,
    mut reader: R,
    writer: &mut W,
) -> CliResult<()> {
    if as_json {
        return Err(invalid_input("--json is not supported with play"));
    }

    let session_id = required_config_value(
        "session id",
        args.session_id.as_deref().or(config.session_id.as_deref()),
    )?;
    let session = SessionServiceClient::new(channel.clone())
        .get_session(with_user(
            GetSessionRequest {
                session_id: session_id.clone(),
            },
            &user_id,
        )?)
        .await?
        .into_inner();

    writeln!(
        writer,
        "session={} title={} game={}",
        session.id, session.title, session.game_id
    )?;
    write_play_help(writer)?;

    let mut line = String::new();
    loop {
        write!(writer, "> ")?;
        writer.flush()?;
        line.clear();
        if reader.read_line(&mut line)? == 0 {
            break;
        }

        let input = line.trim();
        if input.is_empty() {
            continue;
        }

        match handle_play_input(
            channel.clone(),
            &session.id,
            &session.game_id,
            &user_id,
            input,
            args.model.as_deref(),
            writer,
        )
        .await
        {
            Ok(true) => {}
            Ok(false) => break,
            Err(error) => writeln!(writer, "error: {error}")?,
        }
    }

    Ok(())
}

async fn handle_play_input<W: Write>(
    channel: Channel,
    session_id: &str,
    game_id: &str,
    user_id: &str,
    input: &str,
    model: Option<&str>,
    writer: &mut W,
) -> CliResult<bool> {
    match input {
        "/quit" | "/exit" => return Ok(false),
        "/help" => {
            write_play_help(writer)?;
            return Ok(true);
        }
        "/summary" => {
            let summary = MemoryServiceClient::new(channel)
                .get_story_summary(with_user(
                    GetStorySummaryRequest {
                        session_id: session_id.to_owned(),
                    },
                    user_id,
                )?)
                .await?
                .into_inner();
            writeln!(writer, "updated_at={}", summary.updated_at)?;
            writeln!(writer, "{}", summary.content)?;
            return Ok(true);
        }
        "/characters" => {
            let characters = MemoryServiceClient::new(channel)
                .list_characters(with_user(
                    ListCharactersRequest {
                        game_id: game_id.to_owned(),
                        limit: 20,
                        page: None,
                    },
                    user_id,
                )?)
                .await?
                .into_inner()
                .characters;
            for character in characters {
                writeln!(
                    writer,
                    "{}\t{}\tstatus={}\t{}",
                    character.id, character.name, character.status, character.description
                )?;
            }
            return Ok(true);
        }
        "/events" => {
            let events = MemoryServiceClient::new(channel)
                .list_events(with_user(
                    ListEventsRequest {
                        session_id: session_id.to_owned(),
                        limit: 20,
                        page: None,
                    },
                    user_id,
                )?)
                .await?
                .into_inner()
                .events;
            for event in events {
                writeln!(
                    writer,
                    "{}\timportance={}\t{}\t{}",
                    event.id, event.importance, event.created_at, event.summary
                )?;
            }
            return Ok(true);
        }
        _ => {}
    }

    if let Some(content) = input.strip_prefix("/context ") {
        let content = required_value("context content", content)?;
        let response = SessionServiceClient::new(channel)
            .preview_context(with_user(
                PreviewContextRequest {
                    session_id: session_id.to_owned(),
                    content,
                },
                user_id,
            )?)
            .await?
            .into_inner();
        writeln!(writer, "estimated_tokens={}", response.estimated_tokens)?;
        for message in response.messages {
            writeln!(
                writer,
                "{} [{} tokens]\n{}",
                role_name(message.role),
                message.estimated_tokens,
                message.content
            )?;
        }
        return Ok(true);
    }

    if let Some(query) = input.strip_prefix("/memory ") {
        let query = required_value("memory query", query)?;
        let hits = MemoryServiceClient::new(channel)
            .search_memory(with_user(
                SearchMemoryRequest {
                    session_id: session_id.to_owned(),
                    query,
                    limit: 10,
                    page: None,
                },
                user_id,
            )?)
            .await?
            .into_inner()
            .hits;
        for hit in hits {
            writeln!(
                writer,
                "{}\tscore={:.4}\tkind={}\t{}",
                hit.id, hit.score, hit.kind, hit.content
            )?;
        }
        return Ok(true);
    }

    if input.starts_with('/') {
        writeln!(writer, "unknown command: {input}")?;
        return Ok(true);
    }

    super::session::send_message(
        SessionServiceClient::new(channel),
        session_id.to_owned(),
        input.to_owned(),
        model.map(ToOwned::to_owned),
        user_id.to_owned(),
        false,
        writer,
    )
    .await?;
    Ok(true)
}
