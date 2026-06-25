use super::*;

#[tonic::async_trait]
impl memory_service_server::MemoryService for HarpeGrpc {
    type ExportGameStreamStream = ReceiverStream<std::result::Result<pb::GameBackupChunk, Status>>;

    async fn get_story_summary(
        &self,
        request: Request<GetStorySummaryRequest>,
    ) -> std::result::Result<Response<pb::StorySummary>, Status> {
        self.metrics.record_grpc_request();
        let _latency = self.metrics.track_grpc_latency();
        tracing::debug!(rpc = "MemoryService.GetStorySummary");
        let user_id = require_user_id(request.metadata()).map_err(status_from_error)?;
        let request = request.into_inner();
        require_owned_session(self.store.as_ref(), &request.session_id, &user_id)
            .await
            .map_err(status_from_error)?;
        let summary = self
            .store
            .get_story_summary(&request.session_id)
            .await
            .map_err(status_from_error)?
            .ok_or_else(|| Status::not_found(format!("summary {}", request.session_id)))?;

        Ok(Response::new(summary_to_pb(summary)))
    }

    async fn list_characters(
        &self,
        request: Request<ListCharactersRequest>,
    ) -> std::result::Result<Response<pb::ListCharactersResponse>, Status> {
        self.metrics.record_grpc_request();
        let _latency = self.metrics.track_grpc_latency();
        tracing::debug!(rpc = "MemoryService.ListCharacters");
        let user_id = require_user_id(request.metadata()).map_err(status_from_error)?;
        let request = request.into_inner();
        require_owned_game(self.store.as_ref(), &request.game_id, &user_id)
            .await
            .map_err(status_from_error)?;
        let mut characters = self
            .store
            .list_characters(&request.game_id)
            .await
            .map_err(status_from_error)?;
        truncate_to_limit(
            &mut characters,
            request_limit(request.limit, request.page.as_ref()),
        );
        let characters = characters
            .into_iter()
            .map(character_to_pb)
            .collect::<Vec<_>>();
        let page = Some(page_info(characters.len()));

        Ok(Response::new(pb::ListCharactersResponse {
            characters,
            page,
        }))
    }

    async fn get_character(
        &self,
        request: Request<GetCharacterRequest>,
    ) -> std::result::Result<Response<pb::Character>, Status> {
        self.metrics.record_grpc_request();
        let _latency = self.metrics.track_grpc_latency();
        tracing::debug!(rpc = "MemoryService.GetCharacter");
        let user_id = require_user_id(request.metadata()).map_err(status_from_error)?;
        let character = self
            .store
            .get_character(&request.into_inner().character_id)
            .await
            .map_err(status_from_error)?;
        require_owned_game(self.store.as_ref(), &character.game_id, &user_id)
            .await
            .map_err(status_from_error)?;

        Ok(Response::new(character_to_pb(character)))
    }

    async fn list_events(
        &self,
        request: Request<ListEventsRequest>,
    ) -> std::result::Result<Response<pb::ListEventsResponse>, Status> {
        self.metrics.record_grpc_request();
        let _latency = self.metrics.track_grpc_latency();
        tracing::debug!(rpc = "MemoryService.ListEvents");
        let user_id = require_user_id(request.metadata()).map_err(status_from_error)?;
        let request = request.into_inner();
        require_owned_session(self.store.as_ref(), &request.session_id, &user_id)
            .await
            .map_err(status_from_error)?;
        let events = self
            .store
            .list_events(
                &request.session_id,
                request_limit(request.limit, request.page.as_ref()),
            )
            .await
            .map_err(status_from_error)?
            .into_iter()
            .map(event_to_pb)
            .collect::<Vec<_>>();
        let page = Some(page_info(events.len()));

        Ok(Response::new(pb::ListEventsResponse { events, page }))
    }

    async fn list_world_facts(
        &self,
        request: Request<ListWorldFactsRequest>,
    ) -> std::result::Result<Response<pb::ListWorldFactsResponse>, Status> {
        self.metrics.record_grpc_request();
        let _latency = self.metrics.track_grpc_latency();
        tracing::debug!(rpc = "MemoryService.ListWorldFacts");
        let user_id = require_user_id(request.metadata()).map_err(status_from_error)?;
        let request = request.into_inner();
        require_owned_game(self.store.as_ref(), &request.game_id, &user_id)
            .await
            .map_err(status_from_error)?;
        let facts = self
            .store
            .list_world_facts(
                &request.game_id,
                request_limit(request.limit, request.page.as_ref()),
            )
            .await
            .map_err(status_from_error)?
            .into_iter()
            .map(world_fact_to_pb)
            .collect::<Vec<_>>();
        let page = Some(page_info(facts.len()));

        Ok(Response::new(pb::ListWorldFactsResponse { facts, page }))
    }

    async fn list_locations(
        &self,
        request: Request<ListLocationsRequest>,
    ) -> std::result::Result<Response<pb::ListLocationsResponse>, Status> {
        self.metrics.record_grpc_request();
        let _latency = self.metrics.track_grpc_latency();
        tracing::debug!(rpc = "MemoryService.ListLocations");
        let user_id = require_user_id(request.metadata()).map_err(status_from_error)?;
        let request = request.into_inner();
        require_owned_game(self.store.as_ref(), &request.game_id, &user_id)
            .await
            .map_err(status_from_error)?;
        let mut locations = self
            .store
            .list_locations(&request.game_id)
            .await
            .map_err(status_from_error)?;
        truncate_to_limit(
            &mut locations,
            request_limit(request.limit, request.page.as_ref()),
        );
        let locations = locations
            .into_iter()
            .map(location_to_pb)
            .collect::<Vec<_>>();
        let page = Some(page_info(locations.len()));

        Ok(Response::new(pb::ListLocationsResponse { locations, page }))
    }

    async fn search_memory(
        &self,
        request: Request<SearchMemoryRequest>,
    ) -> std::result::Result<Response<pb::SearchMemoryResponse>, Status> {
        self.metrics.record_grpc_request();
        let _latency = self.metrics.track_grpc_latency();
        tracing::debug!(rpc = "MemoryService.SearchMemory");
        let user_id = require_user_id(request.metadata()).map_err(status_from_error)?;
        let request = request.into_inner();
        require_owned_session(self.store.as_ref(), &request.session_id, &user_id)
            .await
            .map_err(status_from_error)?;
        let embedding = self
            .llm
            .embed(&request.query)
            .await
            .map_err(status_from_error)?;
        let hits = self
            .store
            .search_memory(
                &request.session_id,
                &request.query,
                &embedding,
                request_limit(request.limit, request.page.as_ref()),
            )
            .await
            .map_err(status_from_error)?
            .into_iter()
            .map(memory_hit_to_pb)
            .collect::<Vec<_>>();
        let page = Some(page_info(hits.len()));

        Ok(Response::new(pb::SearchMemoryResponse { hits, page }))
    }

    async fn export_game(
        &self,
        request: Request<ExportGameRequest>,
    ) -> std::result::Result<Response<pb::GameSnapshot>, Status> {
        self.metrics.record_grpc_request();
        let _latency = self.metrics.track_grpc_latency();
        tracing::debug!(rpc = "MemoryService.ExportGame");
        let user_id = require_user_id(request.metadata()).map_err(status_from_error)?;
        let request = request.into_inner();
        require_owned_game(self.store.as_ref(), &request.game_id, &user_id)
            .await
            .map_err(status_from_error)?;
        let snapshot = self
            .store
            .export_game_snapshot(&request.game_id)
            .await
            .map_err(status_from_error)?;

        Ok(Response::new(snapshot_to_pb(snapshot)))
    }

    async fn export_game_stream(
        &self,
        request: Request<ExportGameRequest>,
    ) -> std::result::Result<Response<Self::ExportGameStreamStream>, Status> {
        self.metrics.record_grpc_request();
        let _latency = self.metrics.track_grpc_latency();
        tracing::debug!(rpc = "MemoryService.ExportGameStream");
        let user_id = require_user_id(request.metadata()).map_err(status_from_error)?;
        let request = request.into_inner();
        let game = require_owned_game(self.store.as_ref(), &request.game_id, &user_id)
            .await
            .map_err(status_from_error)?;
        let store = self.store.clone();
        let metrics = self.metrics.clone();
        let (tx, rx) = mpsc::channel(16);

        tokio::spawn(async move {
            if let Err(error) = run_export_game_stream(game, store, tx.clone()).await {
                metrics.record_grpc_failure();
                let _ = tx.send(Err(status_from_error(error))).await;
            }
        });

        Ok(Response::new(ReceiverStream::new(rx)))
    }
}
