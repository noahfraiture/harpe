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
- SurrealDB full-text memory indexing plus HNSW vector lookup for 16, 384, 768, 1024, 1536, and 3072-dimensional embeddings
- typed config, graceful shutdown, Docker assets, and snapshot plus streaming game export for backups
- in-process counters and latency histograms with Prometheus text export for gRPC requests, streamed messages, job outcomes, and health checks
- internal admin/debug gRPC methods for background jobs and raw memory chunks
- a Rust CLI client for health checks, users, games, sessions, memory search/views, metrics, backups, and admin debugging
- unit tests plus integration tests covering embedded SurrealDB, migration idempotence, graph edges, and a real gRPC client/server path

## Architecture

Harpe is split into a backend service and thin clients:

- `server/proto/harpe/v1/harpe.proto` defines the gRPC API shared by all clients.
- `harpe-server` hosts the gRPC services, validates ownership through `x-user-id` metadata, builds model context, streams assistant responses, and schedules background memory updates after turns.
- SurrealDB stores users, games, sessions, messages, summaries, characters, events, world facts, locations, graph edges, memory chunks, background jobs, and backup snapshots.
- The LLM layer has a deterministic echo implementation for development/tests and an OpenAI-compatible HTTP adapter for real providers.
- Background jobs update durable memory after assistant turns, with retry/backoff and admin/debug RPCs for failed jobs and raw memory chunks.
- `harpe-cli` provides two terminal clients: the `harpe` command for scripted/admin workflows and `harpe-tui` for interactive roleplay. Future macOS and iOS clients should use the same gRPC API and can copy these workflows: create/select user, game, and session, stream `SendMessage`, and fetch memory/context views as needed.

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

## CLI

The workspace includes a `harpe` command line client:

```sh
cargo run -p harpe-cli -- health
```

Defaults:

- address resolution is `--addr`, then `HARPE_GRPC_ADDR`, then client config, then `http://[::1]:50051`; bare addresses such as `[::1]:50051` are also accepted
- user resolution is `--user-id`, then `HARPE_USER_ID`, then client config
- client config defaults to `$XDG_CONFIG_HOME/harpe/config.toml` or `$HOME/.config/harpe/config.toml`; override with `--config`
- existing `config.json` files are still read as a legacy fallback when `config.toml` is not present
- `--json` switches command output to structured JSON where the command returns a single response

The `user_id` is not an auth token. It is the local backend profile id used to own and filter games. Create it once, save it in config, then omit `--user-id` for normal CLI/TUI usage.

Example config:

```toml
addr = "http://harpe:50051"
user_id = "user_..."
game_id = "game_..."
session_id = "session_..."
```

Basic flow:

```sh
USER_ID=$(cargo run -q -p harpe-cli -- --json user create --name "Noah" | jq -r .id)
cargo run -q -p harpe-cli -- config set-user "$USER_ID"

GAME_ID=$(cargo run -q -p harpe-cli -- --json game create \
  --title "Iron Coast" \
  --system-prompt "Run a tense coastal fantasy adventure." | jq -r .id)
cargo run -q -p harpe-cli -- config set-game "$GAME_ID"

SESSION_ID=$(cargo run -q -p harpe-cli -- --json session create \
  --game "$GAME_ID" \
  --title "First watch" | jq -r .id)
cargo run -q -p harpe-cli -- config set-session "$SESSION_ID"

cargo run -q -p harpe-cli -- play
```

Useful read commands:

```sh
cargo run -q -p harpe-cli -- config show
cargo run -q -p harpe-cli -- game list
cargo run -q -p harpe-cli -- session messages "$SESSION_ID"
cargo run -q -p harpe-cli -- session context "$SESSION_ID" "I inspect the sea gate."
cargo run -q -p harpe-cli -- session send --model gpt-5-mini "$SESSION_ID" "I inspect the sea gate."
cargo run -q -p harpe-cli -- memory search "$SESSION_ID" "sea gate"
cargo run -q -p harpe-cli -- backup stream --game "$GAME_ID" > harpe-backup.ndjson
cargo run -q -p harpe-cli -- metrics export
cargo run -q -p harpe-cli -- admin jobs --status failed
```

Inside `harpe play`, enter normal player messages or slash commands:

```text
/context I inspect the sea gate
/summary
/characters
/events
/memory sea gate
/quit
```

See [cli/README.md](cli/README.md) for focused CLI usage.

## TUI

`harpe-tui` is the richer terminal roleplay cockpit. It uses the same config file as `harpe`, so the user/game/session selected by CLI commands are reused automatically.

```sh
cargo run -q -p harpe-cli --bin harpe-tui -- \
  --addr http://harpe:50051 \
  --user-id <user-id>
```

You can also select the active story directly:

```sh
cargo run -q -p harpe-cli --bin harpe-tui -- \
  --addr http://harpe:50051 \
  --user-id <user-id> \
  --game-id <game-id> \
  --session-id <session-id> \
  --model gpt-5-nano
```

Main keys:

- `Enter`: send the composer as the next player message
- `Alt-Enter` or `Ctrl-J`: insert a newline
- `Ctrl-G`: open the game finder
- `Ctrl-L`: open the session finder
- `Ctrl-T`: switch the right context panel between Cast, Lore, Map, Events, and Context
- `Ctrl-P`: preview the context that would be sent to the model for the current composer text
- `Ctrl-F`: search memory using the composer text
- `Ctrl-R`: refresh session data
- `PageUp` / `PageDown`: scroll the transcript
- `?`: help
- `Ctrl-Q` or `Ctrl-C`: quit

## Test

```sh
cargo test --workspace --locked
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
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
