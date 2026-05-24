# TODO

## Backend

- Add a real LLM provider adapter for chat streaming, embeddings, summarization, and extraction.
- Add token budgeting and model-aware context sizing.
- Move memory extraction, summary updates, and embeddings to background jobs with retries.
- Replace the early schemaless SurrealDB setup with versioned migrations.
- Add DB-side vector indexes, full-text indexes, and graph relation tables.
- Add auth, user ownership, and game/session permissions.
- Add deployment assets, config validation, graceful shutdown, and production observability.
- Add backup/export workflows for campaigns.

## Testing

- Install and wire `cargo-llvm-cov` for numeric coverage reporting.
- Add mocked HTTP tests for the future real LLM provider.
- Add failure-path tests for LLM, database, and streaming errors.
- Add persistent-SurrealDB migration tests outside the in-memory engine.

