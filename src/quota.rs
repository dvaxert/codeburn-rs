//! Live remaining limits for Claude, Codex, and Cursor.
//!
//! Tokens stay in memory for one request and are never logged. A failed
//! provider is an empty list, so the spend JSON still prints.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use chrono::{DateTime, SecondsFormat, Utc};
use rusqlite::{Connection, OpenFlags};
use serde::Serialize;

use crate::scan::Roots;

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Limit {
    pub provider: String,
    pub kind: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub detail: String,
    /// Share already spent, from 0 to 1. The panel draws what is left.
    pub used: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resets_at: Option<String>,
}

pub fn fetch(roots: &Roots) -> Vec<Limit> {
    let mut limits = Vec::new();
    std::thread::scope(|scope| {
        let claude = scope.spawn(|| claude_limits(&roots.claude_dir.join(".credentials.json")));
        let codex = scope.spawn(|| codex_limits(&roots.codex_dir.join("auth.json")));
        let cursor = scope.spawn(|| cursor_limits(&roots.cursor_db));
        for handle in [claude, codex, cursor] {
            if let Ok(rows) = handle.join() {
                limits.extend(rows);
            }
        }
    });
    limits.truncate(24);
    limits
}

fn claude_limits(path: &Path) -> Vec<Limit> {
    let Some(token) = json_string_at(path, &["claudeAiOauth", "accessToken"]) else {
        return Vec::new();
    };
    if let Some(expires_at) = json_i64_at(path, &["claudeAiOauth", "expiresAt"]) {
        if expires_at <= now_ms() {
            return Vec::new();
        }
    }
    let Some(body) = https_get(
        "https://api.anthropic.com/api/oauth/usage",
        &[
            ("Accept", "application/json"),
            ("Authorization", &format!("Bearer {token}")),
            ("anthropic-beta", "oauth-2025-04-20"),
            ("User-Agent", "claude-code/2.1.0"),
        ],
    ) else {
        return Vec::new();
    };
    decode_claude(&body)
}

fn codex_limits(path: &Path) -> Vec<Limit> {
    let Ok(raw) = read_capped(path, 64 * 1024) else {
        return Vec::new();
    };
    let Ok(value) = serde_json::from_str::<serde_json::Value>(&raw) else {
        return Vec::new();
    };
    if value.get("auth_mode").and_then(|item| item.as_str()) != Some("chatgpt") {
        return Vec::new();
    }
    let Some(token) = value
        .pointer("/tokens/access_token")
        .and_then(|item| item.as_str())
        .map(str::trim)
        .map(str::to_string)
    else {
        return Vec::new();
    };
    let account = value
        .pointer("/tokens/account_id")
        .and_then(|item| item.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    let mut headers = vec![
        ("Accept", "application/json".to_string()),
        ("Authorization", format!("Bearer {token}")),
        ("User-Agent", "codeburn-rs".to_string()),
    ];
    if !account.is_empty() {
        headers.push(("ChatGPT-Account-Id", account));
    }
    let header_refs: Vec<(&str, &str)> = headers.iter().map(|(k, v)| (*k, v.as_str())).collect();
    let Some(body) = https_get("https://chatgpt.com/backend-api/wham/usage", &header_refs) else {
        return Vec::new();
    };
    decode_codex(&body)
}

fn cursor_limits(path: &Path) -> Vec<Limit> {
    let Some(token) = cursor_access_token(path) else {
        return Vec::new();
    };
    let Some(cookie) = cursor_session_cookie(&token, now_ms()) else {
        return Vec::new();
    };
    let Some(body) = https_get(
        "https://cursor.com/api/usage-summary",
        &[
            ("Accept", "application/json"),
            ("Cookie", &cookie),
            ("User-Agent", "codeburn-rs"),
        ],
    ) else {
        return Vec::new();
    };
    decode_cursor(&body)
}

pub fn decode_claude(body: &serde_json::Value) -> Vec<Limit> {
    let mut out = Vec::new();
    push_utilization(&mut out, "claude", "five_hour", "", body.get("five_hour"));
    push_utilization(&mut out, "claude", "weekly", "", body.get("seven_day"));
    push_utilization(
        &mut out,
        "claude",
        "weekly_model",
        "Opus",
        body.get("seven_day_opus"),
    );
    push_utilization(
        &mut out,
        "claude",
        "weekly_model",
        "Sonnet",
        body.get("seven_day_sonnet"),
    );
    if let Some(rows) = body.get("limits").and_then(|item| item.as_array()) {
        for row in rows {
            if row.get("kind").and_then(|item| item.as_str()) != Some("weekly_scoped") {
                continue;
            }
            let Some(name) = row
                .pointer("/scope/model/display_name")
                .and_then(|item| item.as_str())
            else {
                continue;
            };
            if out.iter().any(|limit| limit.detail == name) {
                continue;
            }
            let Some(used) = percent_points(row.get("percent")) else {
                continue;
            };
            out.push(limit(
                "claude",
                "weekly_model",
                name,
                used,
                iso_text(row.get("resets_at")),
            ));
        }
    }
    out
}

pub fn decode_codex(body: &serde_json::Value) -> Vec<Limit> {
    let mut out = Vec::new();
    push_codex_window(&mut out, "", body.pointer("/rate_limit/primary_window"));
    push_codex_window(&mut out, "", body.pointer("/rate_limit/secondary_window"));
    if let Some(rows) = body
        .get("additional_rate_limits")
        .and_then(|item| item.as_array())
    {
        for row in rows {
            let Some(name) = row.get("limit_name").and_then(|item| item.as_str()) else {
                continue;
            };
            push_codex_window(&mut out, name, row.pointer("/rate_limit/primary_window"));
            push_codex_window(&mut out, name, row.pointer("/rate_limit/secondary_window"));
        }
    }
    if let Some(window) = spend_window(body) {
        out.push(window);
    }
    out
}

pub fn decode_cursor(body: &serde_json::Value) -> Vec<Limit> {
    let plan = body.pointer("/individualUsage/plan");
    let overall = body.pointer("/individualUsage/overall");
    let pooled = body.pointer("/teamUsage/pooled");
    let reset = iso_text(body.get("billingCycleEnd"));
    let auto = plan.and_then(|row| percent_points(row.get("autoPercentUsed")));
    let api = plan.and_then(|row| percent_points(row.get("apiPercentUsed")));
    let monthly = plan
        .and_then(|row| percent_points(row.get("totalPercentUsed")))
        .or_else(|| match (auto, api) {
            (Some(left), Some(right)) => Some(left.max(right)),
            (Some(value), None) | (None, Some(value)) => Some(value),
            (None, None) => None,
        })
        .or_else(|| plan.and_then(used_over_limit))
        .or_else(|| overall.and_then(used_over_limit))
        .or_else(|| pooled.and_then(used_over_limit))
        .or_else(|| {
            if body.get("isUnlimited").and_then(|item| item.as_bool()) == Some(true) {
                Some(0.0)
            } else {
                None
            }
        });
    let mut out = Vec::new();
    if let Some(used) = monthly {
        out.push(limit("cursor", "monthly", "", used, reset.clone()));
    }
    if let Some(used) = auto {
        out.push(limit("cursor", "auto", "", used, reset.clone()));
    }
    if let Some(used) = api {
        out.push(limit("cursor", "api", "", used, reset.clone()));
    }
    if let Some(used) = on_demand(body.pointer("/individualUsage/onDemand")) {
        out.push(limit("cursor", "on_demand", "", used, reset.clone()));
    }
    if let Some(used) = pooled.and_then(used_over_limit) {
        out.push(limit("cursor", "team", "", used, reset.clone()));
    }
    if monthly.is_some() {
        if let Some(used) = on_demand(body.pointer("/teamUsage/onDemand")) {
            out.push(limit("cursor", "on_demand", "Team", used, reset));
        }
    }
    out
}

fn push_utilization(
    out: &mut Vec<Limit>,
    provider: &str,
    kind: &str,
    detail: &str,
    window: Option<&serde_json::Value>,
) {
    let Some(window) = window else {
        return;
    };
    let Some(used) = percent_points(window.get("utilization")) else {
        return;
    };
    out.push(limit(
        provider,
        kind,
        detail,
        used,
        iso_text(window.get("resets_at")),
    ));
}

fn push_codex_window(out: &mut Vec<Limit>, prefix: &str, window: Option<&serde_json::Value>) {
    let Some(window) = window else {
        return;
    };
    let Some(used) = percent_points(window.get("used_percent")) else {
        return;
    };
    let seconds = window
        .get("limit_window_seconds")
        .and_then(|item| item.as_i64())
        .unwrap_or(0);
    let (kind, generated) = kind_for_seconds(seconds);
    let detail = match (prefix.is_empty(), generated.is_empty()) {
        (true, _) => generated,
        (false, true) => prefix.to_string(),
        (false, false) => format!("{prefix} · {generated}"),
    };
    let resets_at = window
        .get("reset_at")
        .and_then(|item| item.as_i64())
        .and_then(iso_from_unix);
    out.push(limit("codex", kind, &detail, used, resets_at));
}

fn spend_window(body: &serde_json::Value) -> Option<Limit> {
    let row = [
        body.pointer("/spend_control/individual_limit"),
        body.pointer("/spend_control/individualLimit"),
        body.pointer("/individual_limit"),
        body.pointer("/individualLimit"),
        body.pointer("/rate_limit/individual_limit"),
        body.pointer("/rate_limit/individualLimit"),
    ]
    .into_iter()
    .flatten()
    .find(|item| item.is_object())?;
    let limit_value = row.get("limit").and_then(number)?;
    if limit_value <= 0.0 {
        return None;
    }
    let remaining = row
        .get("remaining_percent")
        .or_else(|| row.get("remainingPercent"))
        .and_then(number);
    let used_units = row.get("used").and_then(number);
    let raw = row
        .get("used_percent")
        .or_else(|| row.get("usedPercent"))
        .and_then(number)
        .or_else(|| remaining.map(|value| 100.0 - value))
        .or_else(|| used_units.map(|value| value / limit_value * 100.0))?;
    if !raw.is_finite() || raw < 0.0 {
        return None;
    }
    Some(limit(
        "codex",
        "credits",
        "",
        (raw / 100.0).clamp(0.0, 1.0),
        None,
    ))
}

fn kind_for_seconds(seconds: i64) -> (&'static str, String) {
    if (18_000..19_000).contains(&seconds) {
        return ("five_hour", String::new());
    }
    if (86_400..87_000).contains(&seconds) {
        return ("daily", String::new());
    }
    if (604_800..605_000).contains(&seconds) {
        return ("weekly", String::new());
    }
    if (2_400_000..2_700_000).contains(&seconds) {
        return ("monthly", String::new());
    }
    let label = if seconds <= 0 {
        String::new()
    } else if seconds < 86_400 {
        format!("{}-hour", seconds / 3600)
    } else {
        format!("{}-day", seconds / 86_400)
    };
    ("custom", label)
}

fn on_demand(row: Option<&serde_json::Value>) -> Option<f64> {
    let row = row?;
    if row.get("enabled").and_then(|item| item.as_bool()) == Some(false) {
        return None;
    }
    used_over_limit(row)
}

fn used_over_limit(row: &serde_json::Value) -> Option<f64> {
    let used = number(row.get("used")?)?;
    let limit_value = number(row.get("limit")?)?;
    if used < 0.0 || limit_value <= 0.0 {
        return None;
    }
    Some((used / limit_value).clamp(0.0, 1.0))
}

fn limit(provider: &str, kind: &str, detail: &str, used: f64, resets_at: Option<String>) -> Limit {
    Limit {
        provider: provider.to_string(),
        kind: kind.to_string(),
        detail: detail.to_string(),
        used: (used.clamp(0.0, 1.0) * 10_000.0).round() / 10_000.0,
        resets_at,
    }
}

fn percent_points(value: Option<&serde_json::Value>) -> Option<f64> {
    let number = number(value?)?;
    if number < 0.0 {
        return None;
    }
    Some((number / 100.0).clamp(0.0, 1.0))
}

fn number(value: &serde_json::Value) -> Option<f64> {
    value.as_f64().filter(|number| number.is_finite())
}

fn iso_text(value: Option<&serde_json::Value>) -> Option<String> {
    let text = value?.as_str()?;
    DateTime::parse_from_rfc3339(text)
        .ok()
        .map(|time| time.to_rfc3339_opts(SecondsFormat::Secs, true))
}

fn iso_from_unix(seconds: i64) -> Option<String> {
    DateTime::<Utc>::from_timestamp(seconds, 0)
        .map(|time| time.to_rfc3339_opts(SecondsFormat::Secs, true))
}

fn json_string_at(path: &Path, keys: &[&str]) -> Option<String> {
    let raw = read_capped(path, 64 * 1024).ok()?;
    let value: serde_json::Value = serde_json::from_str(&raw).ok()?;
    let mut cursor = &value;
    for key in keys {
        cursor = cursor.get(*key)?;
    }
    cursor.as_str().map(str::trim).map(str::to_string)
}

fn json_i64_at(path: &Path, keys: &[&str]) -> Option<i64> {
    let raw = read_capped(path, 64 * 1024).ok()?;
    let value: serde_json::Value = serde_json::from_str(&raw).ok()?;
    let mut cursor = &value;
    for key in keys {
        cursor = cursor.get(*key)?;
    }
    cursor.as_i64()
}

fn read_capped(path: &Path, max: u64) -> std::io::Result<String> {
    let meta = fs::metadata(path)?;
    if !meta.is_file() || meta.len() > max {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "credential file is missing or too large",
        ));
    }
    fs::read_to_string(path)
}

fn cursor_access_token(path: &Path) -> Option<String> {
    let uri = format!("file:{}?mode=ro&immutable=1", path.display());
    let conn = Connection::open_with_flags(
        &uri,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI,
    )
    .ok()?;
    let mut token: String = conn
        .query_row(
            "SELECT CAST(value AS TEXT) FROM ItemTable WHERE key = 'cursorAuth/accessToken' LIMIT 1",
            [],
            |row| row.get(0),
        )
        .ok()?;
    token = token.trim().to_string();
    if token.starts_with('"') {
        if let Ok(decoded) = serde_json::from_str::<String>(&token) {
            token = decoded;
        }
    }
    if token.is_empty() || token.starts_with("cursor-safe-storage:") {
        return None;
    }
    Some(token)
}

fn cursor_session_cookie(token: &str, now_ms: i64) -> Option<String> {
    let payload = token.split('.').nth(1)?;
    let bytes = b64url_decode(payload)?;
    let claims: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    let sub = claims.get("sub")?.as_str()?;
    let user_id = sub.split('|').rfind(|part| !part.is_empty())?;
    if !user_id
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-'))
    {
        return None;
    }
    let exp = claims.get("exp")?.as_i64()?;
    if exp * 1000 - now_ms <= 60_000 {
        return None;
    }
    Some(format!("WorkosCursorSessionToken={user_id}%3A%3A{token}"))
}

fn b64url_decode(input: &str) -> Option<Vec<u8>> {
    fn val(byte: u8) -> Option<u8> {
        match byte {
            b'A'..=b'Z' => Some(byte - b'A'),
            b'a'..=b'z' => Some(byte - b'a' + 26),
            b'0'..=b'9' => Some(byte - b'0' + 52),
            b'+' | b'-' => Some(62),
            b'/' | b'_' => Some(63),
            _ => None,
        }
    }
    let mut padded = input.replace('-', "+").replace('_', "/");
    while !padded.len().is_multiple_of(4) {
        padded.push('=');
    }
    let bytes = padded.as_bytes();
    if !bytes.len().is_multiple_of(4) {
        return None;
    }
    let mut out = Vec::with_capacity(bytes.len() / 4 * 3);
    for chunk in bytes.chunks(4) {
        let (a, b) = (val(chunk[0])?, val(chunk[1])?);
        out.push((a << 2) | (b >> 4));
        if chunk[2] != b'=' {
            let c = val(chunk[2])?;
            out.push((b << 4) | (c >> 2));
            if chunk[3] != b'=' {
                out.push((c << 6) | val(chunk[3])?);
            }
        }
    }
    Some(out)
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or(0)
}

fn https_get(url: &str, headers: &[(&str, &str)]) -> Option<serde_json::Value> {
    if !url.starts_with("https://") {
        return None;
    }
    for (name, value) in headers {
        if !header_safe(name) || !header_safe(value) {
            return None;
        }
    }
    let path = secret_config_path()?;
    let _guard = SecretFile(path.clone());
    {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&path)
            .ok()?;
        writeln!(file, "silent").ok()?;
        writeln!(file, "show-error").ok()?;
        writeln!(file, "fail").ok()?;
        writeln!(file, "max-time = 8").ok()?;
        writeln!(file, "connect-timeout = 5").ok()?;
        writeln!(file, "proto = \"=https\"").ok()?;
        writeln!(file, "max-redirs = 0").ok()?;
        for (name, value) in headers {
            writeln!(file, "header = \"{name}: {value}\"").ok()?;
        }
        writeln!(file, "url = \"{url}\"").ok()?;
    }
    let output = Command::new("curl")
        .arg("--config")
        .arg(&path)
        .output()
        .ok()?;
    if !output.status.success() || output.stdout.len() > 1_048_576 {
        return None;
    }
    serde_json::from_slice(&output.stdout).ok()
}

fn header_safe(value: &str) -> bool {
    !value.is_empty()
        && value.len() < 16_384
        && value
            .chars()
            .all(|ch| ch == ' ' || (ch.is_ascii_graphic() && ch != '"' && ch != '\\'))
}

fn secret_config_path() -> Option<PathBuf> {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()?
        .as_nanos();
    Some(std::env::temp_dir().join(format!("codeburn-rs-quota-{}-{nanos}", std::process::id())))
}

struct SecretFile(PathBuf);

impl Drop for SecretFile {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claude_reads_percent_points_and_model_windows() {
        let body = serde_json::json!({
            "five_hour": { "utilization": 25, "resets_at": "2026-11-03T01:00:00Z" },
            "seven_day": { "utilization": 80, "resets_at": "2026-11-09T00:00:00Z" },
            "seven_day_opus": { "utilization": 10, "resets_at": "2026-11-09T00:00:00Z" },
            "limits": [{
                "kind": "weekly_scoped",
                "percent": 40,
                "scope": { "model": { "display_name": "Opus" } },
                "resets_at": "2026-11-09T00:00:00Z"
            }]
        });
        let limits = decode_claude(&body);
        assert_eq!(limits.len(), 3);
        assert_eq!(limits[0].kind, "five_hour");
        assert_eq!(limits[0].used, 0.25);
        assert_eq!(limits[1].kind, "weekly");
        assert_eq!(limits[1].used, 0.8);
        assert_eq!(limits[2].detail, "Opus");
        assert_eq!(limits[2].resets_at.as_deref(), Some("2026-11-09T00:00:00Z"));
    }

    #[test]
    fn codex_labels_windows_by_length() {
        let body = serde_json::json!({
            "rate_limit": {
                "primary_window": { "used_percent": 12.5, "limit_window_seconds": 18000, "reset_at": 1762000000 },
                "secondary_window": { "used_percent": 50, "limit_window_seconds": 604800, "reset_at": 1762000000 }
            }
        });
        let limits = decode_codex(&body);
        assert_eq!(limits[0].kind, "five_hour");
        assert_eq!(limits[0].used, 0.125);
        assert_eq!(limits[1].kind, "weekly");
        assert!(limits[0].resets_at.is_some());
    }

    #[test]
    fn cursor_uses_the_fuller_pool() {
        let body = serde_json::json!({
            "billingCycleEnd": "2026-12-01T00:00:00Z",
            "individualUsage": {
                "plan": { "autoPercentUsed": 20, "apiPercentUsed": 70, "totalPercentUsed": 70 },
                "onDemand": { "enabled": true, "used": 5, "limit": 20 }
            }
        });
        let limits = decode_cursor(&body);
        assert_eq!(limits[0].kind, "monthly");
        assert_eq!(limits[0].used, 0.7);
        assert_eq!(limits[1].kind, "auto");
        assert_eq!(limits[3].kind, "on_demand");
        assert_eq!(limits[3].used, 0.25);
    }

    #[test]
    fn cursor_cookie_needs_a_live_subject() {
        let payload = b64url_encode(br#"{"sub":"auth0|user_01ABC","exp":2000000000}"#);
        let token = format!("aaa.{payload}.sig");
        let cookie = cursor_session_cookie(&token, 1_000).unwrap();
        assert!(cookie.starts_with("WorkosCursorSessionToken=user_01ABC%3A%3A"));
        assert!(cursor_session_cookie(&token, 2_000_000_000_000).is_none());
    }

    fn b64url_encode(bytes: &[u8]) -> String {
        const TABLE: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
        let mut out = String::new();
        for chunk in bytes.chunks(3) {
            let a = chunk[0];
            let b = if chunk.len() > 1 { chunk[1] } else { 0 };
            let c = if chunk.len() > 2 { chunk[2] } else { 0 };
            out.push(TABLE[(a >> 2) as usize] as char);
            out.push(TABLE[(((a & 0x03) << 4) | (b >> 4)) as usize] as char);
            if chunk.len() > 1 {
                out.push(TABLE[(((b & 0x0f) << 2) | (c >> 6)) as usize] as char);
            }
            if chunk.len() > 2 {
                out.push(TABLE[(c & 0x3f) as usize] as char);
            }
        }
        out
    }
}
