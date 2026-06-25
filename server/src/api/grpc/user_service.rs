use super::*;

#[tonic::async_trait]
impl user_service_server::UserService for HarpeGrpc {
    async fn create_user(
        &self,
        request: Request<CreateUserRequest>,
    ) -> std::result::Result<Response<pb::User>, Status> {
        self.metrics.record_grpc_request();
        let _latency = self.metrics.track_grpc_latency();
        tracing::debug!(rpc = "UserService.CreateUser");
        let request = request.into_inner();
        let user = self
            .store
            .create_user(NewUser {
                display_name: request.display_name,
            })
            .await
            .map_err(status_from_error)?;

        Ok(Response::new(user_to_pb(user)))
    }

    async fn get_user(
        &self,
        request: Request<GetUserRequest>,
    ) -> std::result::Result<Response<pb::User>, Status> {
        self.metrics.record_grpc_request();
        let _latency = self.metrics.track_grpc_latency();
        tracing::debug!(rpc = "UserService.GetUser");
        let user = self
            .store
            .get_user(&request.into_inner().user_id)
            .await
            .map_err(status_from_error)?;

        Ok(Response::new(user_to_pb(user)))
    }
}
