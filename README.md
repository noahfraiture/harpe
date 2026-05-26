# Harpe

Backend for an LLM-assisted roleplay game app.

The current milestone is a Rust gRPC server with:

- generated protobuf API for users, games, sessions, messages, summaries, characters, indexed memory search, context preview, health checks, metrics, and game export
- SurrealDB storage through the Rust SDK with versioned migrations, schemafull tables, and graph relation tables
- an LLM abstraction with a deterministic development implementation and an OpenAI-compatible HTTP adapter
- structured memory extraction for events, character updates, world facts, and locations
- a budget-aware context builder that ranks story summary, recent events, memories, character state, world facts, locations, and recent messages
- durable background jobs for turn memory updates with retry/backoff
- `x-user-id` gRPC metadata checks for user-owned game/session data
- SurrealDB full-text memory indexing plus optional HNSW vector lookup for 16-dimensional embeddings
- typed config, graceful shutdown, Docker assets, and game snapshot export for backups
- in-process counters for gRPC requests, streamed messages, job outcomes, and health checks
- unit tests plus integration tests covering embedded SurrealDB, migration idempotence, graph edges, and a real gRPC client/server path

## Run

```sh
cargo run -p harpe-server
```

Defaults:

- `HARPE_GRPC_ADDR=[::1]:50051`
- `SURREALDB_ENDPOINT=memory`
- `SURREALDB_NAMESPACE=harpe`
- `SURREALDB_DATABASE=dev`
- `HARPE_LLM_PROVIDER=echo`
- `HARPE_JOB_INTERVAL_MS=2000`
- `HARPE_JOB_BATCH_LIMIT=25`

For an OpenAI-compatible provider, set:

- `HARPE_LLM_PROVIDER=http`
- `HARPE_LLM_BASE_URL`
- `HARPE_LLM_API_KEY` if the provider requires one
- `HARPE_LLM_CHAT_MODEL`
- `HARPE_LLM_EXTRACTION_MODEL`
- `HARPE_LLM_EMBEDDING_MODEL`

To run the server with SurrealDB:

```sh
docker compose up --build
```

## Test

```sh
cargo test
cargo clippy --all-targets --all-features -- -D warnings
```

The integration tests use SurrealDB's embedded in-memory engine, so no external database is required yet.
