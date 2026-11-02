# codeburn-rs

Local spend from the session files already on disk. The binary is `codeburn-rs`, so it can sit next to the Node `codeburn` without taking its name.

```bash
cargo build --release
./target/release/codeburn-rs status --format menubar-json --period today
```

## What this reads

| Provider | Where | What `status` counts today |
|---|---|---|
| Claude | `~/.claude/projects/**/*.jsonl` (`CODEBURN_RS_CLAUDE_DIR` overrides the `.claude` dir) | JSONL files |
| Codex | `$CODEX_HOME` or `~/.codex`, under `sessions/` and `archived_sessions/` | JSONL files |
| Cursor | `$XDG_CONFIG_HOME/Cursor/User/globalStorage/state.vscdb` (`CODEBURN_RS_CURSOR_DB` overrides the file) | Whether the database is there |

`parsed` is `false` on every provider. `current.cost` stays `0` until a reader prices the files, so a zero is not a bill. Copilot is not a provider.

The Noctalia panel lives in [noctalia-codeburn](https://github.com/dvaxert/noctalia-codeburn).
