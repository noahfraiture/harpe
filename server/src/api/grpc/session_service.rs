use super::*;

#[tonic::async_trait]
impl session_service_server::SessionService for HarpeGrpc {
    type SendMessageStream = ReceiverStream<std::result::Result<MessageDelta, Status>>;

    async fn create_session(
        &self,
        request: Request<CreateSessionRequest>,
    ) -> std::result::Result<Response<pb::Session>, Status> {
        self.metrics.record_grpc_request();
        let _latency = self.metrics.track_grpc_latency();
        tracing::debug!(rpc = "SessionService.CreateSession");
        let user_id = require_user_id(request.metadata()).map_err(status_from_error)?;
        let request = request.into_inner();
        require_owned_game(self.store.as_ref(), &request.game_id, &user_id)
            .await
            .map_err(status_from_error)?;
        let session = self
            .store
            .create_session(NewSession {
                game_id: request.game_id,
                title: request.title,
            })
            .await
            .map_err(status_from_error)?;

        Ok(Response::new(session_to_pb(session)))
    }

    async fn list_sessions(
        &self,
        request: Request<ListSessionsRequest>,
    ) -> std::result::Result<Response<pb::ListSessionsResponse>, Status> {
        self.metrics.record_grpc_request();
        let _latency = self.metrics.track_grpc_latency();
        tracing::debug!(rpc = "SessionService.ListSessions");
        let user_id = require_user_id(request.metadata()).map_err(status_from_error)?;
        let request = request.into_inner();
        require_owned_game(self.store.as_ref(), &request.game_id, &user_id)
            .await
            .map_err(status_from_error)?;
        let sessions = self
            .store
            .list_sessions(
                &request.game_id,
                request_limit(request.limit, request.page.as_ref()),
            )
            .await
            .map_err(status_from_error)?
            .into_iter()
            .map(session_to_pb)
            .collect::<Vec<_>>();
        let page = Some(page_info(sessions.len()));

        Ok(Response::new(pb::ListSessionsResponse { sessions, page }))
    }

    async fn get_session(
        &self,
        request: Request<GetSessionRequest>,
    ) -> std::result::Result<Response<pb::Session>, Status> {
        self.metrics.record_grpc_request();
        let _latency = self.metrics.track_grpc_latency();
        tracing::debug!(rpc = "SessionService.GetSession");
        let user_id = require_user_id(request.metadata()).map_err(status_from_error)?;
        let session_id = request.into_inner().session_id;
        let session = require_owned_session(self.store.as_ref(), &session_id, &user_id)
            .await
            .map_err(status_from_error)?;

        Ok(Response::new(session_to_pb(session)))
    }

    async fn send_message(
        &self,
        request: Request<SendMessageRequest>,
    ) -> std::result::Result<Response<Self::SendMessageStream>, Status> {
        self.metrics.record_grpc_request();
        let _latency = self.metrics.track_grpc_latency();
        tracing::debug!(rpc = "SessionService.SendMessage");
        let user_id = require_user_id(request.metadata()).map_err(status_from_error)?;
        let request = request.into_inner();
        let store = Arc::clone(&self.store);
        let llm = Arc::clone(&self.llm);
        let metrics = Arc::clone(&self.metrics);
        let context_builder = self.context_builder.clone();
        let (tx, rx) = mpsc::channel(16);

        tokio::spawn(async move {
            if let Err(error) = run_send_message(
                request,
                user_id,
                store,
                llm,
                metrics.clone(),
                context_builder,
                tx.clone(),
            )
            .await
            {
                metrics.record_grpc_failure();
                let _ = tx.send(Err(status_from_error(error))).await;
            }
        });

        Ok(Response::new(ReceiverStream::new(rx)))
    }

    async fn preview_context(
        &self,
        request: Request<PreviewContextRequest>,
    ) -> std::result::Result<Response<pb::PreviewContextResponse>, Status> {
        self.metrics.record_grpc_request();
        let _latency = self.metrics.track_grpc_latency();
        tracing::debug!(rpc = "SessionService.PreviewContext");
        let user_id = require_user_id(request.metadata()).map_err(status_from_error)?;
        let request = request.into_inner();
        if request.content.trim().is_empty() {
            return Err(status_from_error(HarpeError::Validation(
                "message content is required".to_owned(),
            )));
        }

        let session = require_owned_session(self.store.as_ref(), &request.session_id, &user_id)
            .await
            .map_err(status_from_error)?;
        let game = self
            .store
            .get_game(&session.game_id)
            .await
            .map_err(status_from_error)?;
        let chat_request = build_context_for_turn(
            &session,
            &game,
            &request.content,
            true,
            self.store.as_ref(),
            self.llm.as_ref(),
            &self.context_builder,
        )
        .await
        .map_err(status_from_error)?;
        let response = preview_context_to_pb(chat_request, &self.context_builder);

        Ok(Response::new(response))
    }

    async fn list_messages(
        &self,
        request: Request<ListMessagesRequest>,
    ) -> std::result::Result<Response<pb::ListMessagesResponse>, Status> {
        self.metrics.record_grpc_request();
        let _latency = self.metrics.track_grpc_latency();
        tracing::debug!(rpc = "SessionService.ListMessages");
        let user_id = require_user_id(request.metadata()).map_err(status_from_error)?;
        let request = request.into_inner();
        require_owned_session(self.store.as_ref(), &request.session_id, &user_id)
            .await
            .map_err(status_from_error)?;
        let messages = self
            .store
            .list_recent_messages(
                &request.session_id,
                request_limit(request.limit, request.page.as_ref()),
            )
            .await
            .map_err(status_from_error)?
            .into_iter()
            .map(message_to_pb)
            .collect::<Vec<_>>();
        let page = Some(page_info(messages.len()));

        Ok(Response::new(pb::ListMessagesResponse { messages, page }))
    }
}
