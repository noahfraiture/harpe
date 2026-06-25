use tonic::metadata::MetadataMap;

use crate::domain::{Game, Session};
use crate::store::HarpeStore;
use crate::{HarpeError, Result};

pub(super) fn optional_user_id(metadata: &MetadataMap) -> Result<Option<String>> {
    let Some(value) = metadata.get("x-user-id") else {
        return Ok(None);
    };

    let value = value
        .to_str()
        .map_err(|_| HarpeError::PermissionDenied("x-user-id metadata is invalid".to_owned()))?
        .trim()
        .to_owned();

    Ok((!value.is_empty()).then_some(value))
}

pub(super) fn require_user_id(metadata: &MetadataMap) -> Result<String> {
    optional_user_id(metadata)?
        .ok_or_else(|| HarpeError::PermissionDenied("x-user-id metadata is required".to_owned()))
}

pub(super) fn resolve_owner_user_id(
    metadata_user_id: Option<&str>,
    request_owner_user_id: &str,
) -> Result<String> {
    let request_owner_user_id = request_owner_user_id.trim();

    match (metadata_user_id, request_owner_user_id.is_empty()) {
        (Some(user_id), true) => Ok(user_id.to_owned()),
        (Some(user_id), false) if user_id == request_owner_user_id => Ok(user_id.to_owned()),
        (Some(_), false) => Err(HarpeError::PermissionDenied(
            "owner_user_id must match x-user-id metadata".to_owned(),
        )),
        (None, false) => Ok(request_owner_user_id.to_owned()),
        (None, true) => Err(HarpeError::PermissionDenied(
            "owner_user_id or x-user-id metadata is required".to_owned(),
        )),
    }
}

pub(super) async fn require_owned_session(
    store: &dyn HarpeStore,
    session_id: &str,
    user_id: &str,
) -> Result<Session> {
    let session = store.get_session(session_id).await?;
    require_owned_game(store, &session.game_id, user_id).await?;

    Ok(session)
}

pub(super) async fn require_owned_game(
    store: &dyn HarpeStore,
    game_id: &str,
    user_id: &str,
) -> Result<Game> {
    let game = store.get_game(game_id).await?;
    if game.owner_user_id == user_id {
        return Ok(game);
    }

    Err(HarpeError::PermissionDenied(format!("game {game_id}")))
}
