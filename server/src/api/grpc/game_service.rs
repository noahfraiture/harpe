use super::*;

#[tonic::async_trait]
impl game_service_server::GameService for HarpeGrpc {
    async fn create_game(
        &self,
        request: Request<CreateGameRequest>,
    ) -> std::result::Result<Response<pb::Game>, Status> {
        self.metrics.record_grpc_request();
        let _latency = self.metrics.track_grpc_latency();
        tracing::debug!(rpc = "GameService.CreateGame");
        let metadata_user_id = optional_user_id(request.metadata()).map_err(status_from_error)?;
        let request = request.into_inner();
        let owner_user_id =
            resolve_owner_user_id(metadata_user_id.as_deref(), &request.owner_user_id)
                .map_err(status_from_error)?;
        let game = self
            .store
            .create_game(NewGame {
                owner_user_id,
                title: request.title,
                system_prompt: request.system_prompt,
            })
            .await
            .map_err(status_from_error)?;

        Ok(Response::new(game_to_pb(game)))
    }

    async fn list_games(
        &self,
        request: Request<ListGamesRequest>,
    ) -> std::result::Result<Response<pb::ListGamesResponse>, Status> {
        self.metrics.record_grpc_request();
        let _latency = self.metrics.track_grpc_latency();
        tracing::debug!(rpc = "GameService.ListGames");
        let user_id = require_user_id(request.metadata()).map_err(status_from_error)?;
        let request = request.into_inner();
        let games = self
            .store
            .list_games_for_user(
                &user_id,
                request_limit(request.limit, request.page.as_ref()),
            )
            .await
            .map_err(status_from_error)?
            .into_iter()
            .map(game_to_pb)
            .collect::<Vec<_>>();
        let page = Some(page_info(games.len()));

        Ok(Response::new(pb::ListGamesResponse { games, page }))
    }

    async fn get_game(
        &self,
        request: Request<GetGameRequest>,
    ) -> std::result::Result<Response<pb::Game>, Status> {
        self.metrics.record_grpc_request();
        let _latency = self.metrics.track_grpc_latency();
        tracing::debug!(rpc = "GameService.GetGame");
        let user_id = require_user_id(request.metadata()).map_err(status_from_error)?;
        let game_id = request.into_inner().game_id;
        let game = require_owned_game(self.store.as_ref(), &game_id, &user_id)
            .await
            .map_err(status_from_error)?;

        Ok(Response::new(game_to_pb(game)))
    }
}
