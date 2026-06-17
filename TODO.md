# TODO

No open backend TODOs.

## CLI Testing Follow-Ups

- Test CLI environment variable wiring:
  - `HARPE_GRPC_ADDR`
  - `HARPE_USER_ID`
- Add broader CLI human-readable output assertions. JSON output is covered more deeply than text output.
- Add CLI-level connection failure tests and more command-specific negative/server error assertions.
- Keep running ignored external tests explicitly when needed:
  - persistent SurrealDB test
  - OpenAI-compatible provider conformance test
- Re-run coverage locally once `llvm-tools-preview` or equivalent `llvm-cov`/`llvm-profdata` binaries are available.
