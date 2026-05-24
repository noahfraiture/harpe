# Harpe

Backend for an LLM-assisted roleplay game app.

The current milestone is a Rust gRPC server with:

- generated protobuf API for games, sessions, messages, summaries, characters, and memory search
- SurrealDB storage through the Rust SDK
- an LLM abstraction with a deterministic development implementation
- structured memory extraction for events, character updates, world facts, and locations
- a context builder that combines system prompt, story summary, recent events, relevant memories, character state, world facts, locations, and recent messages
- unit tests plus integration tests covering embedded SurrealDB and a real gRPC client/server path

## Run

```sh
cargo run -p harpe-server
```

Defaults:

- `HARPE_GRPC_ADDR=[::1]:50051`
- `SURREALDB_ENDPOINT=memory`
- `SURREALDB_NAMESPACE=harpe`
- `SURREALDB_DATABASE=dev`

## Test

```sh
cargo test
cargo clippy --all-targets --all-features -- -D warnings
```

The integration tests use SurrealDB's embedded in-memory engine, so no external database is required yet.
