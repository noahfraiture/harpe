use super::*;

pub(crate) async fn session<W: Write>(
    channel: Channel,
    args: SessionArgs,
    user_id: String,
    config: &ClientConfig,
    as_json: bool,
    writer: &mut W,
) -> CliResult<()> {
    let mut client = SessionServiceClient::new(channel);
    match args.command {
        SessionCommand::Create { game, title } => {
            let game =
                required_config_value("game id", game.as_deref().or(config.game_id.as_deref()))?;
            let response = client
                .create_session(with_user(
                    CreateSessionRequest {
                        game_id: game,
                        title,
                    },
                    &user_id,
                )?)
                .await?
                .into_inner();
            write_session(writer, as_json, &response)
        }
        SessionCommand::List { game, page } => {
            let game =
                required_config_value("game id", game.as_deref().or(config.game_id.as_deref()))?;
            let response = client
                .list_sessions(with_user(
                    ListSessionsRequest {
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
                        "sessions": response.sessions.iter().map(session_json).collect::<Vec<_>>(),
                        "page": page_json(response.page.as_ref()),
                    }),
                )
            } else {
                for session in response.sessions {
                    writeln!(
                        writer,
                        "{}\t{}\tgame={}\tcreated_at={}",
                        session.id, session.title, session.game_id, session.created_at
                    )?;
                }
                Ok(())
            }
        }
        SessionCommand::Get { session_id } => {
            let response = client
                .get_session(with_user(GetSessionRequest { session_id }, &user_id)?)
                .await?
                .into_inner();
            write_session(writer, as_json, &response)
        }
        SessionCommand::Messages { session_id, page } => {
            let response = client
                .list_messages(with_user(
                    ListMessagesRequest {
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
                        "messages": response.messages.iter().map(message_json).collect::<Vec<_>>(),
                        "page": page_json(response.page.as_ref()),
                    }),
                )
            } else {
                for message in response.messages {
                    writeln!(
                        writer,
                        "[{}] {}: {}",
                        message.created_at,
                        role_name(message.role),
                        message.content
                    )?;
                }
                Ok(())
            }
        }
        SessionCommand::Context {
            session_id,
            content,
        } => {
            let response = client
                .preview_context(with_user(
                    PreviewContextRequest {
                        session_id,
                        content: join_words(content),
                    },
                    &user_id,
                )?)
                .await?
                .into_inner();
            if as_json {
                write_json(
                    writer,
                    &json!({
                        "estimated_tokens": response.estimated_tokens,
                        "messages": response.messages.iter().map(context_message_json).collect::<Vec<_>>(),
                    }),
                )
            } else {
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
                Ok(())
            }
        }
        SessionCommand::Send {
            model,
            session_id,
            content,
        } => {
            send_message(
                client,
                session_id,
                join_words(content),
                model,
                user_id,
                as_json,
                writer,
            )
            .await
        }
    }
}

pub(super) async fn send_message<W: Write>(
    mut client: SessionServiceClient<Channel>,
    session_id: String,
    content: String,
    model: Option<String>,
    user_id: String,
    as_json: bool,
    writer: &mut W,
) -> CliResult<()> {
    let mut stream = client
        .send_message(with_user(
            SendMessageRequest {
                session_id,
                content,
                model: normalize_optional_model(model),
            },
            &user_id,
        )?)
        .await?
        .into_inner();
    let mut deltas = Vec::new();
    let mut full_response = String::new();

    while let Some(next) = stream.next().await {
        let delta = next?;
        if as_json {
            full_response.push_str(&delta.delta);
            deltas.push(delta_json(&delta));
        } else if !delta.done {
            write!(writer, "{}", delta.delta)?;
            writer.flush()?;
        }
    }

    if as_json {
        write_json(
            writer,
            &json!({
                "response": full_response,
                "deltas": deltas,
            }),
        )
    } else {
        writeln!(writer)?;
        Ok(())
    }
}
