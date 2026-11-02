# codeburn-rs

[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![Rust 2021](https://img.shields.io/badge/rust-2021-dea584.svg)](https://www.rust-lang.org/)
[![Claude · Codex · Cursor](https://img.shields.io/badge/providers-Claude%20%C2%B7%20Codex%20%C2%B7%20Cursor-111111.svg)](#что-читается)

Локальный учёт расхода на Claude, Codex и Cursor и остаток их лимитов. Бинарь называется `codeburn-rs`, чтобы стоять рядом с npm-пакетом `codeburn` и не занимать его имя. Copilot не читается и в JSON не появляется.

Цифры расхода берутся из сессий, которые уже лежат на диске. Остаток лимита спрашивается у того же входа, которым пользуется сам клиент: токен из локального файла уходит в один запрос и никуда не пишется. Панель для этого JSON — [noctalia-codeburn](https://github.com/dvaxert/noctalia-codeburn).

## Установка

Нужны Rust 2021 и `curl` (им ходят запросы лимитов).

```bash
cargo install --path . --locked
codeburn-rs --version
```

`cargo install` кладёт бинарь в `~/.cargo/bin`. Этот каталог должен быть в `PATH`.

## Команда

```bash
codeburn-rs status --format menubar-json --period today
```

`--format` принимает только `menubar-json`. `--period` задаёт окно расхода:

| Период | Заголовок `current` | График `history` |
|---|---|---|
| `today` | сегодняшний день | последние 14 дней, включая нули |
| `week` | 7 дней, включая сегодня | те же 7 дней |
| `30days` | 30 дней | те же 30 дней |
| `month` | календарный месяц | дни этого месяца |
| `all` | всё, что нашлось | последние 90 дней |

Деньги округляются до 6 знаков. Дни считаются в местной таймзоне. Первый проход по большим JSONL может занять около десяти секунд, повторный читает `~/.cache/codeburn-rs` и обычно укладывается в долю секунды. Три запроса лимитов идут параллельно, каждый обрывается через 8 секунд. Если сервис не ответил или сессии нет, его полос в `limits` просто нет, а расход всё равно печатается.

В `limits` у каждого окна есть `provider`, `kind`, доля уже потраченного `used` (от 0 до 1) и `resetsAt`. Полоса остатка — это `1 - used`. Типичные окна: у Claude 5 часов и неделя, у Codex 5 часов и неделя, у Cursor месяц плюс Auto и API. Отдельного дневного окна у этих тарифов нет.

## Что читается

| | Где | Что идёт в расход |
|---|---|---|
| Claude | `~/.claude/projects/**/*.jsonl` | последнее `usage` каждого сообщения, вместе с кэшем |
| Codex | `$CODEX_HOME` или `~/.codex`, каталоги `sessions/` и `archived_sessions/` | `token_usage_record`, а если его нет — `token_count` |
| Cursor | `$XDG_CONFIG_HOME/Cursor/User/globalStorage/state.vscdb` | последний снимок контекста композера; выходные токены остаются 0 |

Переопределения: `CODEBURN_RS_CLAUDE_DIR`, `CODEX_HOME`, `CODEBURN_RS_CURSOR_DB`.

Цены лежат в `data/prices.json`, это снимок LiteLLM. Неизвестная модель даёт нулевую цену, токены при этом сохраняются. Запись в кэш Claude на 1 час считается по ставке 1.6× от пятиминутной записи. Недостающая ставка записи в кэш берётся как 1.25× входа, чтения — как 0.1× входа.

Лимиты читаются только на чтение:

- Claude — `~/.claude/.credentials.json`, `GET https://api.anthropic.com/api/oauth/usage`
- Codex — `~/.codex/auth.json` в режиме `chatgpt`, `GET https://chatgpt.com/backend-api/wham/usage`
- Cursor — ключ `cursorAuth/accessToken` в той же `state.vscdb`, `GET https://cursor.com/api/usage-summary`

Токен с истёкшим сроком не отправляется. Файл авторизации Codex не перезаписывается.

## Лицензия

[MIT](LICENSE) © 2026 dvaxert
