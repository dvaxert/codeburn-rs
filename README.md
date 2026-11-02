# codeburn-rs

Local spend from the session files already on disk. The binary is `codeburn-rs`, so it can sit next to the Node `codeburn` without taking its name.

```bash
cargo build --release
./target/release/codeburn-rs status --format menubar-json --period today
```

Periods are `today`, `week`, `30days`, `month`, and `all`. The headline follows the period. The chart is the last 14 days for today, the period itself for week, 30 days, and month, and the last 90 days for all.

## What this reads

| Provider | Where | What counts |
|---|---|---|
| Claude | `~/.claude/projects/**/*.jsonl` (`CODEBURN_RS_CLAUDE_DIR` overrides the `.claude` dir) | Last usage on each assistant message, including cache |
| Codex | `$CODEX_HOME` or `~/.codex`, under `sessions/` and `archived_sessions/` | `token_usage_record` when present, otherwise `token_count` |
| Cursor | `$XDG_CONFIG_HOME/Cursor/User/globalStorage/state.vscdb` (`CODEBURN_RS_CURSOR_DB` overrides the file) | Latest composer context per chat. Output stays 0 |

Prices come from the bundled LiteLLM snapshot in `data/prices.json`. A repeated status reuses `~/.cache/codeburn-rs`. Copilot is not a provider.

The Noctalia panel lives in [noctalia-codeburn](https://github.com/dvaxert/noctalia-codeburn).
