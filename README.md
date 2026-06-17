# Harpe

Backend for an LLM-assisted roleplay game app.

The current milestone is a Rust gRPC server with:

- generated protobuf API for users, games, sessions, messages, summaries, characters, indexed memory search, context preview, health checks, metrics, and game export
- SurrealDB storage through the Rust SDK with versioned migrations, schemafull tables, and graph relation tables
- an LLM abstraction with a deterministic development implementation and an OpenAI-compatible HTTP adapter
- structured memory extraction for events, character updates, world facts, and locations
- a model-aware, budget-aware context builder that ranks story summary, recent events, memories, character state, world facts, locations, and recent messages
- durable background jobs for turn memory updates with retry/backoff
- `x-user-id` gRPC metadata checks for user-owned game/session data
- SurrealDB full-text memory indexing plus optional HNSW vector lookup for 16-dimensional embeddings
- typed config, graceful shutdown, Docker assets, and game snapshot export for backups
- in-process counters and latency histograms with Prometheus text export for gRPC requests, streamed messages, job outcomes, and health checks
- internal admin/debug gRPC methods for background jobs and raw memory chunks
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
- `SURREALDB_USERNAME` and `SURREALDB_PASSWORD` are optional; set both when connecting to an authenticated SurrealDB server
- `HARPE_LLM_PROVIDER=echo`
- `HARPE_CONTEXT_MODEL` optionally overrides the model name used for context/token presets
- `HARPE_CONTEXT_WINDOW_TOKENS` optionally overrides the max context window
- `HARPE_RESPONSE_RESERVE_TOKENS` optionally overrides reserved response tokens
- `HARPE_TOKENIZER_PROFILE` optionally forces `generic`, `openai`, `anthropic`, `llama`, or `mistral`
- `HARPE_JOB_INTERVAL_MS=2000`
- `HARPE_JOB_BATCH_LIMIT=25`

For an OpenAI-compatible provider, set:

- `HARPE_LLM_PROVIDER=http`
- `HARPE_LLM_BASE_URL`
- `HARPE_LLM_API_KEY` if the provider requires one
- `HARPE_LLM_CHAT_MODEL`
- `HARPE_LLM_EXTRACTION_MODEL`
- `HARPE_LLM_EMBEDDING_MODEL`
- `HARPE_LLM_TIMEOUT_MS`, default `60000`
- `HARPE_LLM_MAX_RETRIES`, default `2`
- `HARPE_LLM_RETRY_BASE_MS`, default `200`

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

To run the ignored persistent SurrealDB test against the compose database:

```sh
docker compose up -d surrealdb
HARPE_PERSISTENT_SURREALDB_ENDPOINT=ws://127.0.0.1:8000 \
HARPE_PERSISTENT_SURREALDB_USERNAME=root \
HARPE_PERSISTENT_SURREALDB_PASSWORD=root \
cargo test -p harpe-server --test persistent_surreal -- --ignored
```

The GitHub Actions workflow also runs this persistent SurrealDB test on a weekly schedule and on manual dispatch.

To run the ignored OpenAI-compatible provider conformance test, set real provider details:

```sh
HARPE_PROVIDER_CONFORMANCE_BASE_URL=https://provider.example/v1-compatible-root \
HARPE_PROVIDER_CONFORMANCE_API_KEY=... \
HARPE_PROVIDER_CONFORMANCE_CHAT_MODEL=... \
HARPE_PROVIDER_CONFORMANCE_EXTRACTION_MODEL=... \
HARPE_PROVIDER_CONFORMANCE_EMBEDDING_MODEL=... \
cargo test -p harpe-server --test provider_conformance -- --ignored
```
