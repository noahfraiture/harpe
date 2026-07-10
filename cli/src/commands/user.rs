use std::io::Write;

use harpe_proto::pb::{CreateUserRequest, GetUserRequest, user_service_client::UserServiceClient};
use tonic::transport::Channel;

use crate::output::write_user;
use crate::{CliResult, UserArgs, UserCommand};

pub(crate) async fn user<W: Write>(
    channel: Channel,
    args: UserArgs,
    as_json: bool,
    writer: &mut W,
) -> CliResult<()> {
    let mut client = UserServiceClient::new(channel);
    match args.command {
        UserCommand::Create { name } => {
            let response = client
                .create_user(CreateUserRequest { display_name: name })
                .await?
                .into_inner();
            write_user(writer, as_json, &response)
        }
        UserCommand::Get { user_id } => {
            let response = client
                .get_user(GetUserRequest { user_id })
                .await?
                .into_inner();
            write_user(writer, as_json, &response)
        }
    }
}
