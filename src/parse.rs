use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use chrono::{DateTime, Utc};
use rusqlite::{Connection, OpenFlags};
use serde::Deserialize;

use crate::price::{self, codex_buckets};
use crate::scan::Roots;
use crate::usage::{project_name, tokens_of, Call};

const CACHE_VERSION: u32 = 1;

#[derive(Deserialize)]
struct UsageBody {
    input_tokens: Option<f64>,
    output_tokens: Option<f64>,
    cache_creation_input_tokens: Option<f64>,
    cache_read_input_tokens: Option<f64>,
    cached_input_tokens: Option<f64>,
    cache_write_input_tokens: Option<f64>,
    cache_creation: Option<CacheCreation>,
}

#[derive(Deserialize)]
struct CacheCreation {
    ephemeral_1h_input_tokens: Option<f64>,
}

#[derive(Deserialize)]
struct CodexLine {
    timestamp: Option<String>,
    #[serde(rename = "type")]
    kind: String,
    payload: Option<CodexPayload>,
}

#[derive(Deserialize)]
struct CodexPayload {
    #[serde(rename = "type")]
    kind: Option<String>,
    model: Option<String>,
    cwd: Option<String>,
    session_id: Option<String>,
    forked_from_id: Option<String>,
    usage: Option<UsageBody>,
    info: Option<CodexInfo>,
}

#[derive(Deserialize)]
struct CodexInfo {
    last_token_usage: Option<UsageBody>,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct FileCache {
    v: u32,
    len: u64,
    mtime: i64,
    calls: Vec<Call>,
}

pub fn collect(roots: &Roots) -> Vec<Call> {
    let mut seen = HashSet::new();
    let mut calls = Vec::new();
    for path in jsonl_files(&roots.claude_dir.join("projects")) {
        calls.extend(cached_file(&path, &mut seen, parse_claude_file));
    }
    for path in jsonl_files(&roots.codex_dir.join("sessions")) {
        calls.extend(cached_file(&path, &mut seen, parse_codex_file));
    }
    for path in jsonl_files(&roots.codex_dir.join("archived_sessions")) {
        calls.extend(cached_file(&path, &mut seen, parse_codex_file));
    }
    if roots.cursor_db.is_file() {
        calls.extend(parse_cursor(&roots.cursor_db));
    }
    calls
}

fn jsonl_files(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    walk(root, &mut out);
    out
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if kind.is_symlink() {
            continue;
        }
        let path = entry.path();
        if kind.is_dir() {
            walk(&path, out);
        } else if kind.is_file() && path.extension().and_then(|ext| ext.to_str()) == Some("jsonl") {
            out.push(path);
        }
    }
}

fn cached_file(
    path: &Path,
    seen: &mut HashSet<String>,
    parse: fn(&Path, &mut HashSet<String>) -> Vec<Call>,
) -> Vec<Call> {
    let Ok(meta) = fs::metadata(path) else {
        return Vec::new();
    };
    let len = meta.len();
    let mtime = meta
        .modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map(|time| time.as_secs() as i64)
        .unwrap_or(0);
    if let Some(calls) = read_cache(path, len, mtime) {
        for call in &calls {
            seen.insert(dedup_key(call));
        }
        return calls;
    }
    let calls = parse(path, seen);
    write_cache(path, len, mtime, &calls);
    calls
}

fn dedup_key(call: &Call) -> String {
    format!(
        "{}:{}:{}:{}:{}",
        call.provider,
        call.session,
        call.at.to_rfc3339(),
        call.input,
        call.output
    )
}

fn cache_path(path: &Path) -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    let mut hash = 0xcbf29ce484222325u64;
    for byte in path.to_string_lossy().as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    Some(
        PathBuf::from(home)
            .join(".cache/codeburn-rs/v1")
            .join(format!("{hash:016x}.json")),
    )
}

fn read_cache(path: &Path, len: u64, mtime: i64) -> Option<Vec<Call>> {
    let file = cache_path(path)?;
    let text = fs::read_to_string(file).ok()?;
    let cached: FileCache = serde_json::from_str(&text).ok()?;
    if cached.v == CACHE_VERSION && cached.len == len && cached.mtime == mtime {
        Some(cached.calls)
    } else {
        None
    }
}

fn write_cache(path: &Path, len: u64, mtime: i64, calls: &[Call]) {
    let Some(file) = cache_path(path) else {
        return;
    };
    let Some(dir) = file.parent() else {
        return;
    };
    if fs::create_dir_all(dir).is_err() {
        return;
    }
    let body = FileCache {
        v: CACHE_VERSION,
        len,
        mtime,
        calls: calls.to_vec(),
    };
    let Ok(text) = serde_json::to_string(&body) else {
        return;
    };
    let tmp = file.with_extension("json.tmp");
    if fs::write(&tmp, text).is_ok() {
        let _ = fs::rename(tmp, file);
    }
}

fn for_each_line(path: &Path, mut visit: impl FnMut(&str)) {
    let Ok(file) = File::open(path) else {
        return;
    };
    let mut reader = BufReader::new(file);
    let mut buf = Vec::new();
    loop {
        buf.clear();
        match reader.read_until(b'\n', &mut buf) {
            Ok(0) => break,
            Ok(_) => {
                if buf.last() == Some(&b'\n') {
                    buf.pop();
                }
                if buf.last() == Some(&b'\r') {
                    buf.pop();
                }
                if let Ok(line) = std::str::from_utf8(&buf) {
                    visit(line);
                }
            }
            Err(_) => break,
        }
    }
}

fn take_json_object(input: &str) -> Option<&str> {
    let bytes = input.as_bytes();
    if bytes.first() != Some(&b'{') {
        return None;
    }
    let mut depth = 0;
    let mut in_string = false;
    let mut escaped = false;
    for (index, byte) in bytes.iter().enumerate() {
        if in_string {
            if escaped {
                escaped = false;
            } else if *byte == b'\\' {
                escaped = true;
            } else if *byte == b'"' {
                in_string = false;
            }
            continue;
        }
        match *byte {
            b'"' => in_string = true,
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(&input[..=index]);
                }
            }
            _ => {}
        }
    }
    None
}

fn find_usage(line: &str) -> Option<&str> {
    let key = "\"usage\":";
    let mut from = 0;
    while let Some(rel) = line[from..].find(key) {
        let at = from + rel + key.len();
        let rest = &line[at..];
        let skip = rest.bytes().take_while(u8::is_ascii_whitespace).count();
        if let Some(object) = take_json_object(&rest[skip..]) {
            if object.contains("\"input_tokens\"") {
                return Some(object);
            }
        }
        from = at;
    }
    None
}

fn json_string_from(line: &str, key: &str, from: usize) -> Option<String> {
    let hay = line.get(from..)?;
    let pat = format!("\"{key}\":\"");
    let start = hay.find(&pat)? + pat.len();
    let rest = &hay[start..];
    let stop = rest.find('"')?;
    Some(rest[..stop].to_string())
}

fn json_string(line: &str, key: &str, end: Option<usize>) -> Option<String> {
    let hay = end.map(|end| &line[..end.min(line.len())]).unwrap_or(line);
    let pat = format!("\"{key}\":\"");
    let start = hay.rfind(&pat)? + pat.len();
    let rest = &line[start..];
    let stop = rest.find('"')?;
    Some(rest[..stop].to_string())
}

fn parse_claude_file(path: &Path, seen: &mut HashSet<String>) -> Vec<Call> {
    let session = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("claude")
        .to_string();
    let fallback = path
        .parent()
        .and_then(|dir| dir.file_name())
        .and_then(|name| name.to_str())
        .unwrap_or("claude");
    let mut by_id: HashMap<String, Call> = HashMap::new();
    let mut loose = Vec::new();
    for_each_line(path, |line| {
        if !line.contains("\"input_tokens\"") || !line.contains("\"assistant\"") {
            return;
        }
        let Some(usage_json) = find_usage(line) else {
            return;
        };
        let Ok(usage) = serde_json::from_str::<UsageBody>(usage_json) else {
            return;
        };
        let usage_at = line.find(usage_json).unwrap_or(0);
        let after_usage = usage_at + usage_json.len();
        let Some(model) = json_string(line, "model", Some(usage_at)) else {
            return;
        };
        let Some(at) = json_string_from(line, "timestamp", after_usage)
            .or_else(|| json_string(line, "timestamp", Some(usage_at)))
            .and_then(|raw| DateTime::parse_from_rfc3339(&raw).ok())
            .map(|time| time.with_timezone(&Utc))
        else {
            return;
        };
        let project = json_string_from(line, "cwd", after_usage)
            .or_else(|| json_string(line, "cwd", None))
            .map(|cwd| project_name(&cwd))
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| fallback.to_string());
        let write_total = tokens_of(usage.cache_creation_input_tokens);
        let one_hour = tokens_of(
            usage
                .cache_creation
                .and_then(|cache| cache.ephemeral_1h_input_tokens),
        )
        .min(write_total);
        let five_minute = write_total.saturating_sub(one_hour);
        let input = tokens_of(usage.input_tokens);
        let output = tokens_of(usage.output_tokens);
        let cache_read = tokens_of(usage.cache_read_input_tokens);
        if input == 0 && output == 0 && write_total == 0 && cache_read == 0 {
            return;
        }
        let call = Call {
            provider: "claude".to_string(),
            cost: price::cost(
                &model,
                input,
                output,
                five_minute,
                cache_read,
                one_hour,
                false,
            ),
            model,
            project,
            session: session.clone(),
            at,
            input,
            output,
            cache_read,
            cache_write: write_total,
        };
        if let Some(id) = json_string(line, "id", Some(usage_at)) {
            by_id.insert(id, call);
        } else {
            loose.push(call);
        }
    });
    let mut out = Vec::new();
    for (id, call) in by_id {
        if seen.insert(format!("claude:{id}")) {
            out.push(call);
        }
    }
    for call in loose {
        let key = dedup_key(&call);
        if seen.insert(key) {
            out.push(call);
        }
    }
    out
}

fn parse_codex_file(path: &Path, seen: &mut HashSet<String>) -> Vec<Call> {
    let mut session = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("codex")
        .to_string();
    let mut fork = String::new();
    let mut model = String::new();
    let mut project = String::from("codex");
    let mut fast = false;
    let mut records = Vec::new();
    let mut counts = Vec::new();
    for_each_line(path, |line| {
        if line.len() > 1_048_576 || !codex_marker(line) {
            return;
        }
        let Ok(entry) = serde_json::from_str::<CodexLine>(line) else {
            return;
        };
        let Some(payload) = entry.payload else {
            return;
        };
        match entry.kind.as_str() {
            "session_meta" => {
                if let Some(id) = payload.session_id {
                    session = id;
                }
                if let Some(id) = payload.forked_from_id {
                    fork = id;
                }
                if let Some(cwd) = payload.cwd {
                    project = project_name(&cwd);
                }
                if let Some(next) = payload.model {
                    model = next;
                }
            }
            "turn_context" => {
                if let Some(next) = payload.model {
                    model = next;
                }
            }
            "event_msg" if payload.kind.as_deref() == Some("thread_settings_applied") => {
                fast = line.contains("\"service_tier\":\"priority\"")
                    || line.contains("\"service_tier\":\"fast\"");
            }
            "token_usage_record" => {
                if let Some(call) = codex_call(
                    &session,
                    &model,
                    &project,
                    entry.timestamp.as_deref(),
                    payload.usage.as_ref(),
                    fast,
                ) {
                    records.push(call);
                }
            }
            "event_msg" if payload.kind.as_deref() == Some("token_count") => {
                let usage = payload
                    .info
                    .as_ref()
                    .and_then(|info| info.last_token_usage.as_ref());
                if let Some(call) = codex_call(
                    &session,
                    &model,
                    &project,
                    entry.timestamp.as_deref(),
                    usage,
                    fast,
                ) {
                    counts.push(call);
                }
            }
            _ => {}
        }
    });
    let chosen = if records.is_empty() { counts } else { records };
    let mut out = Vec::new();
    for call in chosen {
        let key = format!(
            "codex:{}:{}:{}:{}",
            if fork.is_empty() {
                session.as_str()
            } else {
                fork.as_str()
            },
            call.at.to_rfc3339(),
            call.input,
            call.output
        );
        if seen.insert(key) {
            out.push(call);
        }
    }
    out
}

fn codex_marker(line: &str) -> bool {
    line.contains("\"session_meta\"")
        || line.contains("\"turn_context\"")
        || line.contains("\"token_usage_record\"")
        || line.contains("\"token_count\"")
        || line.contains("\"thread_settings_applied\"")
}

fn codex_call(
    session: &str,
    model: &str,
    project: &str,
    timestamp: Option<&str>,
    usage: Option<&UsageBody>,
    fast: bool,
) -> Option<Call> {
    let usage = usage?;
    let at = DateTime::parse_from_rfc3339(timestamp?)
        .ok()?
        .with_timezone(&Utc);
    let model = if model.is_empty() { "gpt-5" } else { model };
    let input = tokens_of(usage.input_tokens);
    let cached = tokens_of(usage.cached_input_tokens);
    let write = tokens_of(usage.cache_write_input_tokens);
    let output = tokens_of(usage.output_tokens);
    if input == 0 && cached == 0 && output == 0 && write == 0 {
        return None;
    }
    let (billed_input, billed_write, billed_read) = codex_buckets(input, cached, write, model);
    Some(Call {
        provider: "codex".to_string(),
        cost: price::cost(
            model,
            billed_input,
            output,
            billed_write,
            billed_read,
            0,
            fast,
        ),
        model: model.to_string(),
        project: project.to_string(),
        session: session.to_string(),
        at,
        input: billed_input,
        output,
        cache_read: billed_read,
        cache_write: billed_write,
    })
}

fn parse_cursor(path: &Path) -> Vec<Call> {
    let uri = format!("file:{}?mode=ro&immutable=1", path.display());
    let Ok(conn) = Connection::open_with_flags(
        &uri,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI,
    ) else {
        return Vec::new();
    };
    let Ok(mut stmt) = conn.prepare(
        "SELECT
            substr(key, 14),
            json_extract(value, '$.promptTokenBreakdown.totalUsedTokens'),
            json_extract(value, '$.contextTokensUsed'),
            json_extract(value, '$.createdAt'),
            json_extract(value, '$.modelConfig.modelName'),
            json_extract(value, '$.name')
         FROM cursorDiskKV
         WHERE key >= 'composerData:' AND key < 'composerData;'",
    ) else {
        return Vec::new();
    };
    let Ok(rows) = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            flex_f64(row, 1)?,
            flex_f64(row, 2)?,
            flex_f64(row, 3)?,
            row.get::<_, Option<String>>(4)?,
            row.get::<_, Option<String>>(5)?,
        ))
    }) else {
        return Vec::new();
    };
    let mut calls = Vec::new();
    for row in rows.flatten() {
        let (id, used, context, created, model, name) = row;
        let input = tokens_of(used.filter(|n| *n > 0.0).or(context));
        if input == 0 {
            continue;
        }
        let Some(at) = cursor_time(created) else {
            continue;
        };
        let model = model
            .filter(|model| !model.is_empty())
            .unwrap_or_else(|| "cursor-auto".to_string());
        let project = name
            .map(|name| name.chars().take(48).collect::<String>())
            .filter(|name| !name.trim().is_empty())
            .unwrap_or_else(|| "Cursor".to_string());
        calls.push(Call {
            provider: "cursor".to_string(),
            cost: price::cost(&model, input, 0, 0, 0, 0, false),
            model,
            project,
            session: id,
            at,
            input,
            output: 0,
            cache_read: 0,
            cache_write: 0,
        });
    }
    calls
}

fn flex_f64(row: &rusqlite::Row<'_>, index: usize) -> rusqlite::Result<Option<f64>> {
    if let Ok(value) = row.get::<_, Option<i64>>(index) {
        return Ok(value.map(|number| number as f64));
    }
    row.get(index)
}

fn cursor_time(millis: Option<f64>) -> Option<DateTime<Utc>> {
    let millis = millis?;
    let millis = if millis < 10_000_000_000.0 {
        millis * 1000.0
    } else {
        millis
    };
    DateTime::from_timestamp_millis(millis as i64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claude_line_keeps_the_latest_usage_for_one_message() {
        let dir = std::env::temp_dir().join(format!("codeburn-rs-claude-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("session.jsonl");
        fs::write(
            &path,
            concat!(
                "{\"message\":{\"model\":\"claude-opus-5-5\",\"id\":\"msg_a\",\"content\":[{\"type\":\"text\",\"text\":\"hi\"}],\"usage\":{\"input_tokens\":10,\"output_tokens\":1,\"cache_creation_input_tokens\":0,\"cache_read_input_tokens\":0}},\"type\":\"assistant\",\"timestamp\":\"2026-11-02T10:00:00Z\",\"cwd\":\"/home/ex/demo\"}\n",
                "{\"message\":{\"model\":\"claude-opus-5-5\",\"id\":\"msg_a\",\"content\":[{\"type\":\"text\",\"text\":\"hi\"}],\"usage\":{\"input_tokens\":100,\"output_tokens\":4,\"cache_creation_input_tokens\":50,\"cache_read_input_tokens\":25,\"cache_creation\":{\"ephemeral_1h_input_tokens\":50}}},\"type\":\"assistant\",\"timestamp\":\"2026-11-02T10:00:01Z\",\"cwd\":\"/home/ex/demo\"}\n"
            ),
        )
        .unwrap();
        let mut seen = HashSet::new();
        let calls = parse_claude_file(&path, &mut seen);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].input, 100);
        assert_eq!(calls[0].output, 4);
        assert_eq!(calls[0].project, "demo");
        assert!(calls[0].cost > 0.0, "{}", calls[0].cost);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn codex_record_subtracts_cached_input() {
        let dir = std::env::temp_dir().join(format!("codeburn-rs-codex-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("rollout.jsonl");
        fs::write(
            &path,
            concat!(
                "{\"timestamp\":\"2026-11-02T09:00:00Z\",\"type\":\"session_meta\",\"payload\":{\"session_id\":\"s1\",\"cwd\":\"/work/app\",\"model\":\"gpt-5.4\"}}\n",
                "{\"timestamp\":\"2026-11-02T09:01:00Z\",\"type\":\"token_usage_record\",\"payload\":{\"response_id\":\"r1\",\"usage\":{\"input_tokens\":1000,\"cached_input_tokens\":400,\"output_tokens\":20}}}\n",
                "{\"timestamp\":\"2026-11-02T09:01:01Z\",\"type\":\"event_msg\",\"payload\":{\"type\":\"token_count\",\"info\":{\"last_token_usage\":{\"input_tokens\":99999,\"output_tokens\":1}}}}\n"
            ),
        )
        .unwrap();
        let mut seen = HashSet::new();
        let calls = parse_codex_file(&path, &mut seen);
        assert_eq!(calls.len(), 1, "token_count must not double a usage record");
        assert_eq!(calls[0].input, 600);
        assert_eq!(calls[0].cache_read, 400);
        assert_eq!(calls[0].project, "app");
        assert_eq!(calls[0].model, "gpt-5.4");
        assert!(calls[0].cost > 0.0);
        let _ = fs::remove_dir_all(&dir);
    }
}
