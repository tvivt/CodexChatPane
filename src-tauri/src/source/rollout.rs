use super::diagnostics::{Diagnostic, RateLimits};
use super::{seconds_or_millis, ThreadActivity, TokenUsage};
use serde::Deserialize;
use serde_json::Value;
use std::{
    collections::{HashMap, HashSet},
    fs::File,
    io::{BufRead, BufReader, Seek, SeekFrom},
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
    time::SystemTime,
};

// Content is reduced to presence flags; no model/user text is retained or sent to the UI.
#[derive(Deserialize)]
struct Record {
    #[serde(rename = "type", default)]
    kind: String,
    timestamp: Option<String>,
    payload: Payload,
}

#[derive(Deserialize)]
struct Payload {
    #[serde(rename = "type", default)]
    kind: String,
    role: Option<String>,
    turn_id: Option<String>,
    call_id: Option<String>,
    started_at: Option<i64>,
    completed_at: Option<i64>,
    duration_ms: Option<i64>,
    status: Option<String>,
    error: Option<Value>,
    rate_limits: Option<RateLimits>,
    info: Option<TokenInfo>,
    #[serde(default, deserialize_with = "content_present")]
    content: bool,
    #[serde(default, deserialize_with = "content_present")]
    summary: bool,
    #[serde(default, deserialize_with = "content_present")]
    encrypted_content: bool,
    #[serde(default, deserialize_with = "content_present")]
    message: bool,
    #[serde(default, deserialize_with = "content_present")]
    text: bool,
}

#[derive(Deserialize)]
struct TokenInfo {
    total_token_usage: TokenUsage,
}

fn content_present<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<bool, D::Error> {
    fn present(value: &Value) -> bool {
        match value {
            Value::String(text) => !text.trim().is_empty(),
            Value::Array(items) => items.iter().any(present),
            Value::Object(fields) => ["text", "content", "summary", "encrypted_content"]
                .iter()
                .any(|key| fields.get(*key).is_some_and(present)),
            _ => false,
        }
    }
    Value::deserialize(deserializer).map(|value| present(&value))
}

#[derive(Default)]
struct Cursor {
    offset: u64,
    length: u64,
    modified: Option<SystemTime>,
    turn_id: Option<String>,
    activity: Option<ThreadActivity>,
    has_start: bool,
    rate_limits: Option<RateLimits>,
    token_usage: Option<TokenUsage>,
    pending_tools: HashSet<String>,
    pending_message: bool,
}

impl Cursor {
    fn apply(&mut self, record: Record) {
        let payload = record.payload;
        let at = record
            .timestamp
            .as_deref()
            .and_then(super::diagnostics::timestamp);
        if record.kind == "compacted" {
            if let (Some(activity), Some(at)) = (self.activity.as_mut(), at) {
                if activity.working {
                    activity.last_response_at = activity.last_response_at.max(at);
                    if activity.diagnostic.as_ref().is_some_and(|issue| {
                        issue.severity == "warning" && issue.at <= at
                            || issue.kind == "noResponse" && issue.at.saturating_sub(120_000) < at
                    }) {
                        activity.diagnostic = None;
                    }
                }
            }
            return;
        }
        if record.kind == "event_msg" && payload.kind == "token_count" {
            if let Some(info) = payload.info {
                self.token_usage = Some(info.total_token_usage);
            }
            if let (Some(mut limits), Some(at)) = (payload.rate_limits, at) {
                if limits.limit_id.as_deref().is_none_or(|id| id == "codex") {
                    limits.observed_at = at;
                    self.rate_limits = Some(limits);
                }
            }
            return; // Repeated token/limit snapshots are not proof of new model activity.
        }
        let user_message = record.kind == "event_msg" && payload.kind == "user_message"
            || record.kind == "response_item"
                && payload.kind == "message"
                && payload.role.as_deref() == Some("user");
        if user_message {
            if let Some(at) = at {
                if self
                    .activity
                    .as_ref()
                    .is_none_or(|activity| !activity.working && at > activity.activity_at)
                {
                    self.pending_message = self.activity.is_some();
                    self.activity = Some(ThreadActivity {
                        working: true,
                        execution_started_at: Some(at),
                        execution_status: Some("inProgress".into()),
                        activity_at: at,
                        last_user_message_at: at,
                        ..Default::default()
                    });
                    self.turn_id = None;
                    self.has_start = false;
                    self.pending_tools.clear();
                }
                let Some(activity) = self.activity.as_mut() else {
                    return;
                };
                activity.last_user_message_at = activity.last_user_message_at.max(at);
            }
            return;
        }
        if record.kind == "response_item" && payload.kind.ends_with("_output") {
            let current_turn =
                self.activity
                    .as_ref()
                    .is_some_and(|activity| activity.working)
                    && payload.turn_id.as_ref().is_none_or(|turn| {
                        self.turn_id.as_ref().is_none_or(|current| current == turn)
                    });
            if current_turn {
                if let Some(call) = &payload.call_id {
                    self.pending_tools.remove(call);
                } else {
                    self.pending_tools.clear();
                }
                if let (Some(activity), Some(at)) = (self.activity.as_mut(), at) {
                    activity.waiting_for_tool = !self.pending_tools.is_empty();
                    activity.last_response_at = activity.last_response_at.max(at);
                    if activity.diagnostic.as_ref().is_some_and(|issue| {
                        issue.severity == "warning" || issue.kind == "noResponse"
                    }) {
                        activity.diagnostic = None;
                    }
                }
            }
            return;
        }
        let model_activity = record.kind == "response_item"
            && (payload.kind == "message"
                && payload.role.as_deref() == Some("assistant")
                && payload.content
                || payload.kind == "reasoning"
                    && (payload.content || payload.summary || payload.encrypted_content)
                || matches!(
                    payload.kind.as_str(),
                    "function_call"
                        | "custom_tool_call"
                        | "web_search_call"
                        | "local_shell_call"
                        | "image_generation_call"
                ))
            || record.kind == "event_msg"
                && matches!(
                    payload.kind.as_str(),
                    "agent_message" | "agent_reasoning" | "agent_reasoning_raw_content"
                )
                && (payload.message || payload.text);
        if model_activity {
            if let (Some(activity), Some(at)) = (self.activity.as_mut(), at) {
                if activity.working
                    && payload.turn_id.as_ref().is_none_or(|turn| {
                        self.turn_id.as_ref().is_none_or(|current| current == turn)
                    })
                {
                    if matches!(
                        payload.kind.as_str(),
                        "function_call"
                            | "custom_tool_call"
                            | "web_search_call"
                            | "local_shell_call"
                            | "image_generation_call"
                    ) {
                        if let Some(call) = &payload.call_id {
                            self.pending_tools.insert(call.clone());
                        }
                    }
                    activity.waiting_for_tool = !self.pending_tools.is_empty();
                    activity.last_response_at = activity.last_response_at.max(at);
                    if activity.diagnostic.as_ref().is_some_and(|issue| {
                        issue.severity == "warning" && issue.at <= at
                            || issue.kind == "noResponse" && issue.at.saturating_sub(120_000) < at
                    }) {
                        activity.diagnostic = None;
                    }
                }
            }
            return;
        }
        if record.kind != "event_msg"
            || !matches!(
                payload.kind.as_str(),
                "task_started" | "task_complete" | "turn_aborted"
            )
        {
            return;
        }
        let Some(started_at) = payload.started_at.map(seconds_or_millis).or(at) else {
            return;
        };
        let working = payload.kind == "task_started";
        if !working
            && self.pending_message
            && self.activity.as_ref().is_some_and(|activity| {
                activity.working && started_at < activity.last_user_message_at
            })
        {
            return; // A late completion from the preceding turn must not end a newly queued message.
        }
        if !working && self.has_start && self.turn_id.is_some() && payload.turn_id != self.turn_id {
            return; // A delayed terminal event must not stop a newer turn.
        }
        let last_user_message_at = if !working {
            self.activity
                .as_ref()
                .map_or(started_at, |activity| activity.last_user_message_at)
        } else {
            self.activity
                .as_ref()
                .filter(|activity| activity.working && activity.turn_id.is_none())
                .map_or(started_at, |activity| activity.last_user_message_at)
        };
        self.has_start |= working;
        self.pending_message = false;
        self.pending_tools.clear();
        self.turn_id = payload.turn_id;
        let activity_at = if working {
            started_at
        } else {
            payload
                .completed_at
                .map(seconds_or_millis)
                .or(at)
                .unwrap_or(started_at)
        };
        let diagnostic = payload
            .error
            .as_ref()
            .filter(|value| !value.is_null())
            .map(|error| Diagnostic::from_error(error, activity_at, "rollout"));
        let last_response_at = if working {
            0
        } else {
            self.activity
                .as_ref()
                .map_or(0, |activity| activity.last_response_at)
        };
        let execution_ms = if payload.kind == "task_complete" {
            super::execution_duration(
                payload.duration_ms,
                payload.started_at.map(seconds_or_millis).or_else(|| {
                    self.activity
                        .as_ref()
                        .filter(|activity| activity.working)
                        .and_then(|activity| activity.execution_started_at)
                }),
                payload.completed_at.map(seconds_or_millis).or(at),
            )
        } else {
            None
        };
        self.activity = Some(ThreadActivity {
            turn_id: self.turn_id.clone(),
            working,
            last_user_message_at,
            execution_started_at: working.then_some(started_at),
            execution_ms,
            execution_status: Some(
                if working {
                    "inProgress"
                } else if payload.kind == "turn_aborted" {
                    "interrupted"
                } else if diagnostic.is_some() || payload.status.as_deref() == Some("failed") {
                    "failed"
                } else {
                    "completed"
                }
                .to_string(),
            ),
            activity_at,
            diagnostic,
            last_response_at,
            observed: true,
            waiting_for_tool: false,
            token_usage: self.token_usage.clone(),
            ..Default::default()
        });
    }

    fn read(&mut self, file: &mut File, end: u64) -> std::io::Result<()> {
        file.seek(SeekFrom::Start(self.offset))?;
        let mut reader = BufReader::new(file);
        let mut line = String::new();
        while self.offset < end {
            line.clear();
            let read = reader.read_line(&mut line)?;
            if read == 0 || !line.ends_with('\n') {
                break;
            }
            self.offset += read as u64;
            if let Ok(record) = serde_json::from_str(&line) {
                self.apply(record);
            }
        }
        Ok(())
    }
}

pub(super) fn read_rollout_snapshot(
    path: &Path,
    cutoff: Option<i64>,
) -> Result<(Option<ThreadActivity>, Option<RateLimits>), String> {
    static CACHE: OnceLock<Mutex<HashMap<PathBuf, Cursor>>> = OnceLock::new();
    let mut file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok((None, None)),
        Err(error) => return Err(error.to_string()),
    };
    let metadata = file.metadata().map_err(|error| error.to_string())?;
    let length = metadata.len();
    let modified = metadata.modified().ok();
    let mut cache = CACHE
        .get_or_init(Default::default)
        .lock()
        .map_err(|error| error.to_string())?;
    let cursor = cache.entry(path.to_path_buf()).or_default();
    if cursor.modified.is_none()
        || length < cursor.length
        || length == cursor.length && modified != cursor.modified
    {
        // Bootstrap from the tail; grow only until the latest task start is found.
        let mut window = 65536;
        loop {
            *cursor = Cursor::default();
            cursor.offset = length.saturating_sub(window);
            if cursor.offset > 0 {
                file.seek(SeekFrom::Start(cursor.offset))
                    .map_err(|error| error.to_string())?;
                let skipped = BufReader::new(&mut file)
                    .read_until(b'\n', &mut Vec::new())
                    .map_err(|error| error.to_string())?;
                cursor.offset += skipped as u64;
            }
            cursor
                .read(&mut file, length)
                .map_err(|error| error.to_string())?;
            let old_terminal = cursor.activity.as_ref().is_some_and(|activity| {
                !activity.working
                    && cutoff.is_some_and(|cutoff| activity.activity_at < seconds_or_millis(cutoff))
            });
            if cursor.has_start || cursor.pending_message || old_terminal || window >= length {
                break;
            }
            window = window.saturating_mul(2);
        }
    } else if length > cursor.length {
        cursor
            .read(&mut file, length)
            .map_err(|error| error.to_string())?;
    }
    cursor.length = length;
    cursor.modified = modified;
    let mut activity = cursor.activity.clone();
    if let Some(activity) = activity.as_mut() {
        activity.token_usage = cursor.token_usage.clone();
        activity.observed = true;
        if activity.working
            && !cutoff.is_some_and(|cutoff| {
                activity.execution_started_at.unwrap_or(0) >= seconds_or_millis(cutoff)
            })
        {
            activity.working = false;
            activity.execution_started_at = None;
            activity.execution_status = None;
        }
    }
    Ok((activity, cursor.rate_limits.clone()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn read_rollout_activity(
        path: &Path,
        cutoff: Option<i64>,
    ) -> Result<Option<ThreadActivity>, String> {
        read_rollout_snapshot(path, cutoff).map(|snapshot| snapshot.0)
    }

    #[test]
    fn compaction_counts_as_activity_without_hiding_a_real_error() {
        let mut cursor = Cursor::default();
        cursor.apply(serde_json::from_value(serde_json::json!({"type":"event_msg","timestamp":"2026-08-31T10:00:00Z","payload":{"type":"task_started","turn_id":"compact"}})).unwrap());
        let at = super::super::diagnostics::timestamp("2026-08-31T10:02:00Z").unwrap();
        cursor
            .activity
            .as_mut()
            .unwrap()
            .update_diagnostic(None, at);
        assert_eq!(
            cursor
                .activity
                .as_ref()
                .unwrap()
                .diagnostic
                .as_ref()
                .unwrap()
                .kind,
            "noResponse"
        );
        cursor.apply(serde_json::from_value(serde_json::json!({"type":"compacted","timestamp":"2026-08-31T10:02:01Z","payload":{}})).unwrap());
        cursor
            .activity
            .as_mut()
            .unwrap()
            .update_diagnostic(None, at + 2000);
        assert!(cursor.activity.as_ref().unwrap().diagnostic.is_none());
        cursor.activity.as_mut().unwrap().diagnostic = Some(Diagnostic::from_error(
            &serde_json::json!({"message":"request timed out"}),
            at + 3000,
            "rollout",
        ));
        cursor.apply(serde_json::from_value(serde_json::json!({"type":"compacted","timestamp":"2026-08-31T10:02:04Z","payload":{}})).unwrap());
        assert_eq!(
            cursor
                .activity
                .as_ref()
                .unwrap()
                .diagnostic
                .as_ref()
                .unwrap()
                .kind,
            "network"
        );
    }

    #[test]
    fn tracks_real_response_and_embedded_error_without_counting_placeholders() {
        let mut cursor = Cursor::default();
        let apply = |cursor: &mut Cursor, kind: &str, payload: Value, second: u8| {
            cursor.apply(serde_json::from_value(serde_json::json!({"type": kind, "timestamp": format!("2026-08-31T10:00:{second:02}Z"), "payload": payload})).unwrap());
        };
        apply(
            &mut cursor,
            "event_msg",
            serde_json::json!({"type":"task_started","turn_id":"a"}),
            0,
        );
        apply(
            &mut cursor,
            "response_item",
            serde_json::json!({"type":"reasoning","summary":[],"content":[]}),
            1,
        );
        apply(
            &mut cursor,
            "event_msg",
            serde_json::json!({"type":"token_count","rate_limits":{"limit_id":"codex","primary":{"used_percent":9,"window_minutes":300,"resets_at":1788191001}}}),
            2,
        );
        assert_eq!(cursor.activity.as_ref().unwrap().last_response_at, 0);
        assert_eq!(
            cursor
                .rate_limits
                .as_ref()
                .unwrap()
                .primary
                .as_ref()
                .unwrap()
                .used_percent,
            9.0
        );
        apply(
            &mut cursor,
            "response_item",
            serde_json::json!({"type":"reasoning","summary":[{"type":"summary_text","text":"working"}]}),
            3,
        );
        assert!(cursor.activity.as_ref().unwrap().last_response_at > 0);
        apply(
            &mut cursor,
            "event_msg",
            serde_json::json!({"type":"task_complete","turn_id":"a","error":{"codex_error_info":"usage_limit_exceeded","message":"limit reached"}}),
            4,
        );
        assert_eq!(
            cursor
                .activity
                .as_ref()
                .unwrap()
                .execution_status
                .as_deref(),
            Some("failed")
        );
        assert_eq!(
            cursor
                .activity
                .as_ref()
                .unwrap()
                .diagnostic
                .as_ref()
                .unwrap()
                .kind,
            "quota"
        );
        apply(
            &mut cursor,
            "event_msg",
            serde_json::json!({"type":"task_started","turn_id":"b"}),
            5,
        );
        assert!(cursor.activity.as_ref().unwrap().diagnostic.is_none());
        assert_eq!(cursor.activity.as_ref().unwrap().last_response_at, 0);
        apply(
            &mut cursor,
            "response_item",
            serde_json::json!({"type":"function_call","name":"exec_command","arguments":"{}"}),
            6,
        );
        assert!(cursor.activity.as_ref().unwrap().last_response_at > 0);
    }

    #[test]
    fn tail_without_start_uses_latest_terminal() {
        let mut cursor = Cursor::default();
        for (kind, turn_id) in [("turn_aborted", "old"), ("task_complete", "new")] {
            cursor.apply(
                serde_json::from_value(serde_json::json!({
                    "type": "event_msg", "timestamp": "2026-08-31T04:32:52Z",
                    "payload": { "type": kind, "turn_id": turn_id }
                }))
                .unwrap(),
            );
        }
        let activity = cursor.activity.unwrap();
        assert_eq!(activity.turn_id.as_deref(), Some("new"));
        assert_eq!(activity.execution_status.as_deref(), Some("completed"));
    }

    #[test]
    fn follows_appends_partial_lines_and_replaced_rollout() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("rollout.jsonl");
        let start = "{\"type\":\"event_msg\",\"payload\":{\"type\":\"task_started\",\"turn_id\":\"new\",\"started_at\":1788150770}}\n";
        std::fs::write(&path, start).unwrap();
        assert!(
            read_rollout_activity(&path, Some(1788150000))
                .unwrap()
                .unwrap()
                .working
        );
        let mut append = File::options().append(true).open(&path).unwrap();
        writeln!(append, "{{\"type\":\"response_item\",\"timestamp\":\"2026-08-31T04:32:52.123Z\",\"payload\":{{\"type\":\"message\",\"role\":\"user\"}}}}").unwrap();
        writeln!(append, "{{\"type\":\"event_msg\",\"payload\":{{\"type\":\"turn_aborted\",\"turn_id\":\"old\",\"started_at\":1788150000}}}}").unwrap();
        write!(append, "{{\"type\":\"event_msg\",\"payload\":{{\"type\":\"task_complete\",\"turn_id\":\"new\",\"started_at\":1788150770,\"completed_at\":1788150780,\"duration_ms\":10000}}").unwrap();
        let active = read_rollout_activity(&path, Some(1788150000))
            .unwrap()
            .unwrap();
        assert!(active.working);
        assert_eq!(active.last_user_message_at, 1788150772123);
        writeln!(append, "}}").unwrap();
        let completed = read_rollout_activity(&path, Some(1788150000))
            .unwrap()
            .unwrap();
        assert_eq!(completed.execution_status.as_deref(), Some("completed"));
        assert_eq!(completed.execution_ms, Some(10000));
        assert_eq!(completed.last_user_message_at, active.last_user_message_at);
        std::fs::write(&path, start).unwrap();
        assert!(
            read_rollout_activity(&path, Some(1788150000))
                .unwrap()
                .unwrap()
                .working
        );
        assert!(
            !read_rollout_activity(&path, Some(1788151000))
                .unwrap()
                .unwrap()
                .working
        );
    }

    #[test]
    fn handles_tail_user_records_queued_messages_and_pending_tools() {
        let mut cursor = Cursor::default();
        let apply = |cursor: &mut Cursor, timestamp: &str, kind: &str, payload: Value| {
            cursor.apply(
                serde_json::from_value(
                    serde_json::json!({"timestamp":timestamp,"type":kind,"payload":payload}),
                )
                .unwrap(),
            );
        };
        apply(
            &mut cursor,
            "1970-01-01T00:02:05Z",
            "event_msg",
            serde_json::json!({"type":"user_message"}),
        );
        apply(
            &mut cursor,
            "1970-01-01T00:02:10Z",
            "event_msg",
            serde_json::json!({"type":"task_complete","started_at":120,"completed_at":130}),
        );
        assert_eq!(
            cursor
                .activity
                .as_ref()
                .unwrap()
                .execution_status
                .as_deref(),
            Some("completed")
        );
        apply(
            &mut cursor,
            "1970-01-01T00:02:20Z",
            "event_msg",
            serde_json::json!({"type":"user_message"}),
        );
        assert!(cursor.pending_message);
        apply(
            &mut cursor,
            "1970-01-01T00:02:21Z",
            "event_msg",
            serde_json::json!({"type":"task_complete","started_at":120,"completed_at":130}),
        );
        assert!(cursor.activity.as_ref().unwrap().working);
        apply(
            &mut cursor,
            "1970-01-01T00:02:30Z",
            "event_msg",
            serde_json::json!({"type":"task_started","turn_id":"new"}),
        );
        assert_eq!(
            cursor.activity.as_ref().unwrap().last_user_message_at,
            140000
        );
        apply(
            &mut cursor,
            "1970-01-01T00:02:31Z",
            "response_item",
            serde_json::json!({"type":"function_call","call_id":"tool-a"}),
        );
        apply(
            &mut cursor,
            "1970-01-01T00:02:32Z",
            "event_msg",
            serde_json::json!({"type":"user_message"}),
        );
        let activity = cursor.activity.as_mut().unwrap();
        assert!(activity.waiting_for_tool);
        activity.update_diagnostic(None, 400000);
        assert!(activity.diagnostic.is_none());
        apply(
            &mut cursor,
            "1970-01-01T00:02:33Z",
            "response_item",
            serde_json::json!({"type":"function_call_output","call_id":"tool-a","error":{"message":"tool timed out"}}),
        );
        let activity = cursor.activity.as_mut().unwrap();
        assert!(!activity.waiting_for_tool);
        assert_eq!(activity.last_response_at, 153000);
        activity.update_diagnostic(None, 272999);
        assert!(activity.diagnostic.is_none());
        activity.update_diagnostic(None, 273000);
        assert_eq!(activity.diagnostic.as_ref().unwrap().kind, "noResponse");
    }

    #[test]
    fn keeps_latest_cumulative_token_usage() {
        let mut cursor = Cursor::default();
        cursor.apply(
            serde_json::from_value(serde_json::json!({
                "type":"event_msg",
                "timestamp":"2026-09-02T10:00:00Z",
                "payload":{"type":"token_count","info":{"total_token_usage":{
                    "input_tokens":142300,"cached_input_tokens":98100,
                    "output_tokens":21600,"reasoning_output_tokens":8400
                }}}
            }))
            .unwrap(),
        );
        let usage = cursor.token_usage.unwrap();
        assert_eq!(usage.input_tokens, 142300);
        assert_eq!(usage.cached_input_tokens, 98100);
        assert_eq!(usage.output_tokens, 21600);
        assert_eq!(usage.reasoning_output_tokens, 8400);
    }
}
