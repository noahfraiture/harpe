use super::*;

pub(crate) async fn game<W: Write>(
    channel: Channel,
    args: GameArgs,
    user_id: String,
    as_json: bool,
    writer: &mut W,
) -> CliResult<()> {
    let mut client = GameServiceClient::new(channel);
    match args.command {
        GameCommand::Create {
            title,
            system_prompt,
            system_prompt_file,
        } => {
            let response = client
                .create_game(with_user(
                    CreateGameRequest {
                        title,
                        system_prompt: read_prompt(system_prompt, system_prompt_file)?,
                        owner_user_id: String::new(),
                    },
                    &user_id,
                )?)
                .await?
                .into_inner();
            write_game(writer, as_json, &response)
        }
        GameCommand::List { page } => {
            let response = client
                .list_games(with_user(
                    ListGamesRequest {
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
                        "games": response.games.iter().map(game_json).collect::<Vec<_>>(),
                        "page": page_json(response.page.as_ref()),
                    }),
                )
            } else {
                for game in response.games {
                    writeln!(
                        writer,
                        "{}\t{}\towner={}\tcreated_at={}",
                        game.id, game.title, game.owner_user_id, game.created_at
                    )?;
                }
                Ok(())
            }
        }
        GameCommand::Get { game_id } => {
            let response = client
                .get_game(with_user(GetGameRequest { game_id }, &user_id)?)
                .await?
                .into_inner();
            write_game(writer, as_json, &response)
        }
    }
}
