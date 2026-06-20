# Harpe CLI

`harpe` is the terminal client for the Harpe gRPC backend.
`harpe-tui` is the interactive terminal roleplay cockpit built on the same gRPC API and config file.

Run the backend first:

```sh
cargo run -p harpe-server
```

Run the client through Cargo:

```sh
cargo run -p harpe-cli -- health
```

## Configuration

The client resolves settings in this order:

- address: `--addr`, `HARPE_GRPC_ADDR`, client config, `http://[::1]:50051`
- user: `--user-id`, `HARPE_USER_ID`, client config
- config path: `--config`, `HARPE_CONFIG`, `$XDG_CONFIG_HOME/harpe/config.json`, `$HOME/.config/harpe/config.json`

Common config commands:

```sh
harpe config show
harpe config set-addr http://[::1]:50051
harpe config set-user <user-id>
harpe config set-game <game-id>
harpe config set-session <session-id>
harpe config clear session
```

When running through Cargo, put client arguments after `--`:

```sh
cargo run -q -p harpe-cli -- config show
```

## Basic Flow

```sh
USER_ID=$(cargo run -q -p harpe-cli -- --json user create --name "Noah" | jq -r .id)
cargo run -q -p harpe-cli -- config set-user "$USER_ID"

GAME_ID=$(cargo run -q -p harpe-cli -- --json game create \
  --title "Iron Coast" \
  --system-prompt "Run a tense coastal fantasy adventure." | jq -r .id)
cargo run -q -p harpe-cli -- config set-game "$GAME_ID"

SESSION_ID=$(cargo run -q -p harpe-cli -- --json session create \
  --title "First watch" | jq -r .id)
cargo run -q -p harpe-cli -- config set-session "$SESSION_ID"

cargo run -q -p harpe-cli -- play
```

## Play Mode

`play` opens a simple roleplay loop for the selected session:

```sh
harpe play
harpe play <session-id>
```

Normal input is sent as the next player message and streams the assistant response. Slash commands inspect backend state:

```text
/context I inspect the sea gate
/summary
/characters
/events
/memory sea gate
/help
/quit
```

## TUI Cockpit

Run the richer terminal UI through Cargo:

```sh
cargo run -q -p harpe-cli --bin harpe-tui -- \
  --addr http://harpe:50051 \
  --user-id <user-id>
```

Or pin the story and model for the session:

```sh
cargo run -q -p harpe-cli --bin harpe-tui -- \
  --addr http://harpe:50051 \
  --user-id <user-id> \
  --game-id <game-id> \
  --session-id <session-id> \
  --model gpt-5-nano
```

The screen is organized as a roleplay cockpit:

- header: active game, session, selected model, current user, and backend health
- left panel on wide terminals: current scene, location, open threads, and recent lore
- center: transcript
- right panel: Cast, Lore, Map, Events, and Context tabs
- bottom: multiline composer

Key bindings:

```text
Enter             send message
Alt-Enter/Ctrl-J  newline
Ctrl-G            game finder
Ctrl-L            session finder
Ctrl-T            switch context panel
Ctrl-P            preview model context
Ctrl-M            search memory from composer text
Ctrl-R            refresh active data
PageUp/PageDown   scroll transcript
?                 help
Ctrl-Q/Ctrl-C     quit
```

## Useful Commands

```sh
harpe health
harpe metrics
harpe metrics export
harpe user create --name "Noah"
harpe game create --title "Iron Coast" --system-prompt-file prompt.txt
harpe game list
harpe session create --title "First watch"
harpe session messages <session-id>
harpe session context <session-id> "I inspect the sea gate."
harpe session send <session-id> "I inspect the sea gate."
harpe session send --model gpt-5-mini <session-id> "I inspect the sea gate."
harpe memory summary <session-id>
harpe memory search <session-id> "sea gate"
harpe backup export --out backup.json
harpe backup stream --out backup.ndjson
harpe admin jobs --status failed
harpe admin memory-chunks <session-id>
```
