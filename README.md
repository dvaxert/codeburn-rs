<h1 align="center">🔥 codeburn-rs</h1>

<p align="center"><strong>See where your Claude, Codex, and Cursor spend goes.</strong></p>

<p align="center">
  <a href="https://github.com/dvaxert/codeburn-rs/actions/workflows/ci.yml"><img src="https://github.com/dvaxert/codeburn-rs/actions/workflows/ci.yml/badge.svg" alt="CI" /></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-MIT-F97316.svg" alt="MIT license" /></a>
  <a href="https://www.rust-lang.org/"><img src="https://img.shields.io/badge/rust-2021-F97316?logo=rust&logoColor=white" alt="Rust 2021" /></a>
  <a href="#sixty-seconds"><img src="https://img.shields.io/badge/platform-Linux-F97316?logo=linux&logoColor=white" alt="Linux" /></a>
</p>

<p align="center">
  <a href="#what-it-reads"><img src="https://img.shields.io/badge/Claude_Code-spend_%2B_limits-555555?labelColor=F97316" alt="Claude Code: spend and limits" /></a>
  <a href="#what-it-reads"><img src="https://img.shields.io/badge/Codex-spend_%2B_limits-555555?labelColor=F97316" alt="Codex: spend and limits" /></a>
  <a href="#what-it-reads"><img src="https://img.shields.io/badge/Cursor-spend_%2B_limits-555555?labelColor=F97316" alt="Cursor: spend and limits" /></a>
</p>

<p align="center">
  <a href="https://github.com/getagentseal/codeburn"><img src="https://img.shields.io/badge/rewrite_of-CodeBurn-F97316?logo=github&logoColor=white" alt="Rust rewrite of CodeBurn" /></a>
  <a href="https://github.com/dvaxert/noctalia-codeburn"><img src="https://img.shields.io/badge/panel-Noctalia-F97316.svg" alt="Noctalia panel" /></a>
</p>

<p align="center"><code>codeburn-rs status --period today</code></p>

<p align="center"><sub>A Rust rewrite of part of <a href="https://github.com/getagentseal/codeburn">CodeBurn</a> by AgentSeal. The binary is named <code>codeburn-rs</code> so it can sit next to npm <code>codeburn</code>.</sub></p>

A Rust reader for the session files Claude Code, Codex, and Cursor already write. It prices those sessions and asks each signed-in client how much of the plan is left, then prints one JSON document. Copilot is not a provider.

The same JSON drives the [Noctalia panel](https://github.com/dvaxert/noctalia-codeburn).

## Compared to CodeBurn

[CodeBurn](https://github.com/getagentseal/codeburn) is the original and the much larger tool. This one does a small part of its job. Pick CodeBurn if you want a terminal dashboard or desktop apps, or use other tools.

| | codeburn-rs | [CodeBurn](https://github.com/getagentseal/codeburn) |
|---|---|---|
| Tools covered | 3 | 42 |
| Interface | JSON, plus a [Noctalia plugin](https://github.com/dvaxert/noctalia-codeburn) for the bar and panel | interactive dashboard, desktop and menu bar apps |
| Runs on | Linux | macOS, Windows, Linux |
| Needs | `curl` for live limits | Node.js 22.13+ |
| Ships as | one binary of about 4 MB | npm package, Homebrew, installers |

## Sixty seconds

On Arch and its derivatives, build a pacman package:

```bash
git clone https://github.com/dvaxert/codeburn-rs
cd codeburn-rs
makepkg -si
codeburn-rs status --format menubar-json --period today
```

To get the [Noctalia plugin](https://github.com/dvaxert/noctalia-codeburn) as well, run its `install.sh` instead. It installs both.

Elsewhere, use `cargo install --path . --locked`. Remove it with `pacman -R codeburn-rs` or `cargo uninstall codeburn-rs`.

`cargo install` puts the binary in `~/.cargo/bin`. That directory needs to be on `PATH`. The bundled SQLite needs a C compiler to build. Live limits also need `curl`.

<details>
<summary>What the JSON looks like</summary>

Shortened, with made-up numbers:

```json
{
  "generated": "2026-11-02T16:59:09Z",
  "period": "today",
  "providers": [
    { "name": "claude", "located": true, "files": 412, "parsed": true, "cost": 12.34, "calls": 318, "sessions": 6 }
  ],
  "current": {
    "label": "Today",
    "cost": 18.92,
    "calls": 455,
    "sessions": 9,
    "topModels": [{ "name": "claude-opus-5-5", "cost": 12.34, "tokens": 8120000 }],
    "projects": [{ "name": "codeburn-rs", "cost": 4.2, "tokens": 2310000 }]
  },
  "limits": [
    { "provider": "claude", "kind": "five_hour", "used": 0.27, "resetsAt": "2026-11-02T19:49:59Z" }
  ],
  "history": {
    "label": "14 days",
    "daily": [{ "date": "2026-10-20", "cost": 7.5 }]
  }
}
```

</details>

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

## Development

```bash
cargo fmt --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --locked
```

CI runs the same three commands on every push to `main` and on pull requests.

## License

[MIT](LICENSE). Notices for the projects below are in [THIRD-PARTY-NOTICES.md](THIRD-PARTY-NOTICES.md).

Based on [CodeBurn](https://github.com/getagentseal/codeburn) by AgentSeal, and rewritten in Rust for three of its providers. It is a separate project, not affiliated with CodeBurn. Pricing data from [LiteLLM](https://github.com/BerriAI/litellm).
