# TODO

## Backend

- Replace the first-pass token estimator with model/provider-specific tokenization.
- Add model-aware context sizing presets.
- Add dead-letter retry/purge commands for permanently failed background jobs.
- Add provider-specific vector indexes once production embedding dimensions/model choices settle.
- Backfill richer graph relation edges from extraction batches, such as event-character, event-location, character-fact, and memory-fact links.
- Add production observability: request latency histograms, metrics export format, and distributed tracing spans.
- Add incremental/streaming backup export for large campaigns.
- Add more scripted memory eval scenarios for social scenes, combat, inventory, and long travel arcs.

## Testing

- Add a manual or scheduled CI job that runs the ignored persistent SurrealDB test against a real container.
- Add provider-conformance tests for each real LLM provider once model choices settle.
