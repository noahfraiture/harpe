use tonic::{Request, Response, Status};

use crate::domain::NewUser;
use crate::pb::{self, CreateUserRequest, GetUserRequest, user_service_server};

use super::HarpeGrpc;
use super::convert::{status_from_error, user_to_pb};

#[tonic::async_trait]
impl user_service_server::UserService for HarpeGrpc {
    async fn create_user(
        &self,
        request: Request<CreateUserRequest>,
    ) -> std::result::Result<Response<pb::User>, Status> {
        let _request = self.observe_request("UserService.CreateUser");
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
        let _request = self.observe_request("UserService.GetUser");
        let user = self
            .store
            .get_user(&request.into_inner().user_id)
            .await
            .map_err(status_from_error)?;

        Ok(Response::new(user_to_pb(user)))
    }
}
