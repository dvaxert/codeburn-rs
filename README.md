<p align="center">
  <a href="#what-it-reads"><img src="https://raw.githubusercontent.com/getagentseal/codeburn/main/assets/providers/claude.jpg" alt="Claude" height="34" /></a>
  &nbsp;
  <a href="#what-it-reads"><img src="https://raw.githubusercontent.com/getagentseal/codeburn/main/assets/providers/codex.png" alt="Codex" height="34" /></a>
  &nbsp;
  <a href="#what-it-reads"><img src="https://raw.githubusercontent.com/getagentseal/codeburn/main/assets/providers/cursor.jpg" alt="Cursor" height="34" /></a>
</p>

<p align="center"><strong>See where your Claude, Codex, and Cursor spend goes.</strong></p>

<p align="center">
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-MIT-F97316.svg" alt="MIT license" /></a>
  <a href="https://www.rust-lang.org/"><img src="https://img.shields.io/badge/rust-2021-F97316.svg" alt="Rust 2021" /></a>
  <a href="https://github.com/dvaxert/noctalia-codeburn"><img src="https://img.shields.io/badge/panel-Noctalia-F97316.svg" alt="Noctalia panel" /></a>
</p>

<p align="center"><code>codeburn-rs status --format menubar-json --period today</code></p>

<p align="center"><sub>Install with <code>cargo install --path . --locked</code>. The binary is named <code>codeburn-rs</code> so it can sit next to npm <code>codeburn</code>.</sub></p>

A Rust reader for the session files Claude Code, Codex, and Cursor already write. It prices those sessions and asks each signed-in client how much of the plan is left. Copilot is not a provider.

The same JSON drives the [Noctalia panel](https://github.com/dvaxert/noctalia-codeburn).

## Sixty seconds

```bash
cargo install --path . --locked
codeburn-rs status --format menubar-json --period today
```

`cargo install` puts the binary in `~/.cargo/bin`. That directory needs to be on `PATH`. Live limits also need `curl`.

There is no account. The command reads the folders those three tools already use, prices every token it can, and prints one JSON document.

`--format` accepts only `menubar-json`. `--period` picks the spend window. The chart is not always the same window as the headline: today keeps a 14-day chart so the line is not flat.

| Period | Headline `current` | Chart `history` |
|---|---|---|
| `today` | today | the last 14 days, zeros included |
| `week` | 7 days, including today | those same 7 days |
| `30days` | 30 days | those same 30 days |
| `month` | the calendar month | the days of that month |
| `all` | everything on disk | the last 90 days |

Money is rounded to 6 decimal places. Days use the local timezone. The first pass over large JSONL files can take about ten seconds. The next one reads `~/.cache/codeburn-rs` and is usually a fraction of a second. The three limit requests run in parallel and each stops after 8 seconds. A provider that does not answer is simply missing from `limits`. The spend still prints.

## What it reads

| | Where | What counts |
|---|---|---|
| Claude | `~/.claude/projects/**/*.jsonl` | the last `usage` on each assistant message, cache included |
| Codex | `$CODEX_HOME` or `~/.codex`, under `sessions/` and `archived_sessions/` | `token_usage_record` when the file has one, otherwise `token_count` |
| Cursor | `$XDG_CONFIG_HOME/Cursor/User/globalStorage/state.vscdb` | the latest composer context for each chat. Output stays 0 |

Overrides: `CODEBURN_RS_CLAUDE_DIR`, `CODEX_HOME`, `CODEBURN_RS_CURSOR_DB`.

Prices ship in `data/prices.json`, a LiteLLM snapshot. A model with no row costs 0 and its tokens are still counted. A one-hour Claude cache write is billed at 1.6× the five-minute cache-write rate. A missing cache-write rate falls back to 1.25× input, and a missing cache-read rate to 0.1× input.

## Stay ahead

`limits` is the live reading. Each window has `provider`, `kind`, `used` as a fraction from 0 to 1, and `resetsAt`. The bar for what is left is `1 - used`.

Claude reports a 5-hour window and a weekly window. Codex reports a 5-hour window and a weekly window. Cursor reports the billing month, plus Auto and API when the payload has them. These plans do not expose a separate daily window.

The requests are read-only and use the login the app already stored:

- Claude reads `~/.claude/.credentials.json` and calls `GET https://api.anthropic.com/api/oauth/usage`
- Codex reads `~/.codex/auth.json` when `auth_mode` is `chatgpt` and calls `GET https://chatgpt.com/backend-api/wham/usage`
- Cursor reads `cursorAuth/accessToken` from the same `state.vscdb` and calls `GET https://cursor.com/api/usage-summary`

An expired token is not sent. The Codex auth file is never rewritten. Tokens are not logged.

## Private by design

Spend comes from files already on disk. Limit calls are the only network, and they send the credential that tool already keeps locally. Prompts and source are not uploaded. CodeBurn-rs does not sit between you and the agent.

## License

[MIT](LICENSE).

Pricing data from [LiteLLM](https://github.com/BerriAI/litellm). This tree is a Rust reader for three of the providers in [CodeBurn](https://github.com/getagentseal/codeburn). It is not that project.
