use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::{HashMap, HashSet},
    fs::{self, File},
    io::{BufRead, BufReader, Seek, SeekFrom},
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
    time::SystemTime,
};
use time::{format_description::well_known::Rfc3339, OffsetDateTime};

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Diagnostic {
    pub kind: String,
    pub severity: String,
    pub message: String,
    pub at: i64,
    pub source: String,
    pub retry: Option<u64>,
    pub max_retries: Option<u64>,
    pub resets_at: Option<i64>,
}

impl Diagnostic {
    pub(super) fn from_error(value: &Value, at: i64, source: &str) -> Self {
        let message = value
            .get("message")
            .and_then(Value::as_str)
            .or_else(|| value.as_str())
            .unwrap_or("Codex 返回失败，但未记录错误原因")
            .to_string();
        let code = value
            .get("codexErrorInfo")
            .or_else(|| value.get("codex_error_info"))
            .unwrap_or(&Value::Null)
            .to_string()
            .to_ascii_lowercase()
            .replace('_', "");
        let lower = message.to_ascii_lowercase();
        let kind = if code.contains("usagelimitexceeded")
            || lower.starts_with("you've hit your usage limit")
        {
            "quota"
        } else if [
            "httpconnectionfailed",
            "responsestreamconnectionfailed",
            "responsestreamdisconnected",
            "responsetoomanyfailedattempts",
        ]
        .iter()
        .any(|name| code.contains(name))
            || [
                "stream disconnected",
                "connection reset",
                "connection refused",
                "network error",
                "request timed out",
            ]
            .iter()
            .any(|text| lower.contains(text))
        {
            "network"
        } else if lower.starts_with("invalid prompt:")
            && (lower.contains("usage policy") || lower.contains("flagged"))
        {
            "promptRejected"
        } else {
            "other"
        };
        Self {
            kind: kind.into(),
            severity: "error".into(),
            message,
            at,
            source: source.into(),
            retry: None,
            max_retries: None,
            resets_at: value
                .get("resets_at")
                .or_else(|| value.get("resetsAt"))
                .and_then(Value::as_i64),
        }
    }

    pub(super) fn no_response(since: i64) -> Self {
        Self {
            kind: "noResponse".into(),
            severity: "error".into(),
            message: "上次有效模型或工具进展后 120 秒未有新进展；工具执行期间不计入此判断。".into(),
            at: since + 120_000,
            source: "activity-watch".into(),
            retry: None,
            max_retries: None,
            resets_at: None,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct RateWindow {
    pub used_percent: f64,
    pub window_minutes: i64,
    pub resets_at: Option<i64>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct RateLimits {
    pub limit_id: Option<String>,
    pub primary: Option<RateWindow>,
    pub secondary: Option<RateWindow>,
    pub plan_type: Option<String>,
    #[serde(default)]
    pub observed_at: i64,
}

pub(super) fn timestamp(value: &str) -> Option<i64> {
    OffsetDateTime::parse(value, &Rfc3339)
        .ok()
        .map(|at| (at.unix_timestamp_nanos() / 1_000_000) as i64)
}

// Only warnings that identify the model request and exact turn are relevant.
fn retry_record(line: &str) -> Option<(String, Diagnostic)> {
    if !line.contains("hostId=local") {
        return None;
    }
    let encoded = line.split_once("error=")?.1;
    let outer = serde_json::Deserializer::from_str(encoded)
        .into_iter::<Value>()
        .next()?
        .ok()?;
    let inner: Value = serde_json::from_str(outer.get("message")?.as_str()?).ok()?;
    if inner.get("target")?.as_str()? != "codex_core::responses_retry" {
        return None;
    }
    let fields = inner.get("fields")?;
    let turn = fields.get("turn_id")?.as_str()?.to_string();
    let message = fields
        .get("sampling_error")
        .or_else(|| fields.get("message"))?
        .as_str()?
        .to_string();
    let at = timestamp(inner.get("timestamp")?.as_str()?)?;
    Some((
        turn,
        Diagnostic {
            kind: "retry".into(),
            severity: "warning".into(),
            message,
            at,
            source: "desktop-log".into(),
            retry: fields.get("retries").and_then(Value::as_u64),
            max_retries: fields.get("max_retries").and_then(Value::as_u64),
            resets_at: None,
        },
    ))
}

#[derive(Default)]
struct LogCursor {
    offset: u64,
    length: u64,
    modified: Option<SystemTime>,
}
#[derive(Default)]
struct RetryCache {
    files: HashMap<PathBuf, LogCursor>,
    turns: HashMap<String, Diagnostic>,
}

pub(super) fn read_retries(
    root: &Path,
    active_turns: &HashSet<String>,
    cutoff: Option<i64>,
) -> HashMap<String, Diagnostic> {
    static CACHE: OnceLock<Mutex<RetryCache>> = OnceLock::new();
    let Ok(mut cache) = CACHE.get_or_init(Default::default).lock() else {
        return HashMap::new();
    };
    cache.turns.retain(|turn, _| active_turns.contains(turn));
    if active_turns.is_empty() || cutoff.is_none() {
        return HashMap::new();
    }
    let mut present = HashSet::new();
    for date in [
        OffsetDateTime::now_utc(),
        OffsetDateTime::now_utc() - time::Duration::days(1),
    ] {
        let folder = root.join(format!(
            "{:04}/{:02}/{:02}",
            date.year(),
            date.month() as u8,
            date.day()
        ));
        let Ok(entries) = fs::read_dir(folder) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|v| v.to_str()) != Some("log") {
                continue;
            }
            present.insert(path.clone());
            let Ok(mut file) = File::open(&path) else {
                continue;
            };
            let Ok(metadata) = file.metadata() else {
                continue;
            };
            let length = metadata.len();
            let modified = metadata.modified().ok();
            let cursor = cache.files.entry(path).or_default();
            if cursor.modified == modified && cursor.length == length {
                continue;
            }
            if cursor.modified.is_none() || length < cursor.length || length == cursor.length {
                // ponytail: bootstrap the last 512 KiB only; persistent live events are read incrementally.
                cursor.offset = length.saturating_sub(512 * 1024);
                if file.seek(SeekFrom::Start(cursor.offset)).is_err() {
                    continue;
                }
                if cursor.offset > 0 {
                    let Ok(skipped) = BufReader::new(&mut file).read_until(b'\n', &mut Vec::new())
                    else {
                        continue;
                    };
                    cursor.offset += skipped as u64;
                }
            }
            if file.seek(SeekFrom::Start(cursor.offset)).is_err() {
                continue;
            }
            let mut reader = BufReader::new(file);
            let mut line = String::new();
            let mut events = Vec::new();
            while cursor.offset < length {
                line.clear();
                let Ok(read) = reader.read_line(&mut line) else {
                    break;
                };
                if read == 0 || !line.ends_with('\n') {
                    break;
                }
                cursor.offset += read as u64;
                if let Some((turn, diagnostic)) = retry_record(&line) {
                    if active_turns.contains(&turn)
                        && diagnostic.at >= super::seconds_or_millis(cutoff.unwrap())
                    {
                        events.push((turn, diagnostic));
                    }
                }
            }
            cursor.length = length;
            cursor.modified = modified;
            for (turn, diagnostic) in events {
                if cache
                    .turns
                    .get(&turn)
                    .is_none_or(|old| diagnostic.at > old.at)
                {
                    cache.turns.insert(turn, diagnostic);
                }
            }
        }
    }
    cache.files.retain(|path, _| present.contains(path));
    cache.turns.clone()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn retry_log_reader_follows_appends_without_crossing_turns() {
        use std::io::Write;
        let directory = tempfile::tempdir().unwrap();
        let now = OffsetDateTime::now_utc();
        let folder = directory.path().join(format!(
            "{:04}/{:02}/{:02}",
            now.year(),
            now.month() as u8,
            now.day()
        ));
        fs::create_dir_all(&folder).unwrap();
        let path = folder.join("desktop.log");
        let inner = json!({"timestamp":now.format(&Rfc3339).unwrap(),"target":"codex_core::responses_retry","fields":{"turn_id":"retry-fixture","sampling_error":"connection closed","retries":2,"max_retries":5}});
        let line = format!(
            "error={} hostId=local",
            json!({"message":inner.to_string()})
        );
        fs::write(&path, &line).unwrap();
        let active = HashSet::from(["retry-fixture".to_string()]);
        let cutoff = Some(now.unix_timestamp() - 1);
        assert!(read_retries(directory.path(), &active, cutoff).is_empty());
        writeln!(File::options().append(true).open(&path).unwrap()).unwrap();
        let result = read_retries(directory.path(), &active, cutoff);
        assert_eq!(result["retry-fixture"].retry, Some(2));
        assert_eq!(read_retries(directory.path(), &active, cutoff).len(), 1);
        assert!(read_retries(
            directory.path(),
            &HashSet::from(["another-turn".to_string()]),
            cutoff
        )
        .is_empty());
    }

    #[test]
    fn classifies_errors_and_scopes_network_retries() {
        for (value, kind) in [
            (
                json!({"codex_error_info":"usage_limit_exceeded","message":"limit"}),
                "quota",
            ),
            (
                json!({"codexErrorInfo":{"responseStreamDisconnected":{"httpStatusCode":502}},"message":"lost"}),
                "network",
            ),
            (
                json!({"codexErrorInfo":"other","message":"Invalid prompt: flagged by our usage policy"}),
                "promptRejected",
            ),
            (
                json!({"codexErrorInfo":"other","message":"unrelated error"}),
                "other",
            ),
        ] {
            assert_eq!(Diagnostic::from_error(&value, 1, "test").kind, kind);
        }
        let inner = json!({"timestamp":"2026-08-31T10:00:00Z","target":"codex_core::responses_retry","fields":{"turn_id":"turn-a","sampling_error":"connection closed","retries":1,"max_retries":5}});
        let line = format!(
            "info error={} hostId=local state=connected",
            json!({"message":inner.to_string()})
        );
        let (turn, issue) = retry_record(&line).unwrap();
        assert_eq!(turn, "turn-a");
        assert_eq!(issue.retry, Some(1));
        assert_eq!(issue.max_retries, Some(5));
        assert!(retry_record(&line.replace("hostId=local", "hostId=remote")).is_none());
        assert!(retry_record(&line.replace("codex_core::responses_retry", "statsig")).is_none());
    }
}
