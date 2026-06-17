use harpe_server::db::surreal::{SurrealCredentials, SurrealStore};
use harpe_server::domain::{NewGame, NewSession, NewUser};
use harpe_server::store::HarpeStore;
use uuid::Uuid;

#[tokio::test]
#[ignore = "requires a persistent SurrealDB server; see README"]
async fn persistent_surrealdb_migrations_are_idempotent_and_data_survives_reconnect() {
    let endpoint = env_or("HARPE_PERSISTENT_SURREALDB_ENDPOINT", "ws://127.0.0.1:8000");
    let namespace = env_or("HARPE_PERSISTENT_SURREALDB_NAMESPACE", "harpe_test");
    let database = env_or(
        "HARPE_PERSISTENT_SURREALDB_DATABASE",
        &format!("persistent_test_{}", Uuid::now_v7()),
    );
    let credentials = persistent_credentials();

    let store = SurrealStore::connect_with_credentials(
        endpoint.clone(),
        &namespace,
        &database,
        credentials.clone(),
    )
    .await
    .unwrap();
    let first_migrations = store.applied_migrations().await.unwrap();
    assert_eq!(
        first_migrations
            .iter()
            .map(|migration| migration.version)
            .collect::<Vec<_>>(),
        SurrealStore::migration_versions()
    );
    store.migrate().await.unwrap();
    let second_migrations = store.applied_migrations().await.unwrap();
    assert_eq!(second_migrations.len(), first_migrations.len());

    let user = store
        .create_user(NewUser {
            display_name: "Persistent Eval".to_owned(),
        })
        .await
        .unwrap();
    let game = store
        .create_game(NewGame {
            owner_user_id: user.id.clone(),
            title: "Persistent Coast".to_owned(),
            system_prompt: "Persist this campaign.".to_owned(),
        })
        .await
        .unwrap();
    let session = store
        .create_session(NewSession {
            game_id: game.id,
            title: "Persistent Session".to_owned(),
        })
        .await
        .unwrap();

    let reconnected =
        SurrealStore::connect_with_credentials(endpoint, &namespace, &database, credentials)
            .await
            .unwrap();
    assert_eq!(reconnected.get_user(&user.id).await.unwrap().id, user.id);
    assert_eq!(
        reconnected.get_session(&session.id).await.unwrap().id,
        session.id
    );
}

fn env_or(key: &str, default: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| default.to_owned())
}

fn persistent_credentials() -> Option<SurrealCredentials> {
    if std::env::var("HARPE_PERSISTENT_SURREALDB_NO_AUTH").as_deref() == Ok("1") {
        return None;
    }

    Some(SurrealCredentials {
        username: env_or("HARPE_PERSISTENT_SURREALDB_USERNAME", "root"),
        password: env_or("HARPE_PERSISTENT_SURREALDB_PASSWORD", "root"),
    })
}
