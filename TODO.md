# TODO

## Backend

- Replace the first-pass token estimator with model/provider-specific tokenization.
- Add model-aware context sizing presets.
- Add automatic retry/backoff policies for failed background jobs.
- Add DB-side vector indexes and full-text indexes once embedding dimensions/model choices settle.
- Backfill richer graph relation edges from extraction batches, such as event-character, event-location, character-fact, and memory-fact links.
- Add production observability: structured request logs, metrics, tracing spans, and health checks.
- Add incremental/streaming backup export for large campaigns.

## Testing

- Install and wire `cargo-llvm-cov` for numeric coverage reporting.
- Add failure-path tests for LLM, database, and streaming errors.
- Add persistent-SurrealDB migration tests outside the in-memory engine.
