# TODO

## Backend

- Replace the first-pass token estimator with model/provider-specific tokenization.
- Add model-aware context sizing presets.
- Add dead-letter management UI/commands for permanently failed background jobs.
- Add provider-specific vector indexes once production embedding dimensions/model choices settle.
- Backfill richer graph relation edges from extraction batches, such as event-character, event-location, character-fact, and memory-fact links.
- Add production observability: request latency histograms, metrics export format, and distributed tracing spans.
- Add incremental/streaming backup export for large campaigns.

## Testing

- Install and wire `cargo-llvm-cov` for numeric coverage reporting.
- Add failure-path tests for LLM, database, and streaming errors.
- Add persistent-SurrealDB migration tests outside the in-memory engine.
