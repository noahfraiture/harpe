use tonic::Request;
use tonic::metadata::MetadataValue;

use crate::CliResult;

pub(crate) fn with_user<T>(message: T, user_id: &str) -> CliResult<Request<T>> {
    let mut request = Request::new(message);
    request
        .metadata_mut()
        .insert("x-user-id", MetadataValue::try_from(user_id)?);
    Ok(request)
}
