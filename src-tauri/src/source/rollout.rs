use super::diagnostics::Diagnostic;
use super::{questions::Questions, seconds_or_millis, ThreadActivity};
use crate::rollout_log;
use serde::Deserialize;
use serde_json::Value;
use std::{
    collections::{HashMap, HashSet},
    io::{BufRead, BufReader, Read},
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
    time::SystemTime,
};
mod generation;
pub(crate) use generation::Generation;

// Message text is inspected only while parsing; none is retained or sent to the status UI.
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
    id: Option<String>,
    thread_id: Option<String>,
    model: Option<String>,
    effort: Option<String>,
    thread_settings: Option<Value>,
    role: Option<String>,
    turn_id: Option<String>,
    call_id: Option<String>,
    response_id: Option<String>,
    compaction_response_id: Option<String>,
    name: Option<String>,
    arguments: Option<String>,
    item: Option<Value>,
    started_at: Option<i64>,
    completed_at: Option<i64>,
    duration_ms: Option<i64>,
    status: Option<String>,
    error: Option<Value>,
    #[serde(default)]
    content: Value,
    #[serde(default, deserialize_with = "content_present")]
    summary: bool,
    #[serde(default, deserialize_with = "content_present")]
    encrypted_content: bool,
    #[serde(default)]
    message: Value,
    #[serde(default, deserialize_with = "content_present")]
    text: bool,
}

fn content_present<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<bool, D::Error> {
    Value::deserialize(deserializer).map(|value| content_has_text(&value))
}

fn content_has_text(value: &Value) -> bool {
    match value {
        Value::String(text) => !text.trim().is_empty(),
        Value::Array(items) => items.iter().any(content_has_text),
        Value::Object(fields) => ["text", "content", "summary", "encrypted_content"]
            .iter()
            .any(|key| fields.get(*key).is_some_and(content_has_text)),
        _ => false,
    }
}

#[derive(Default)]
struct Cursor {
    generation: generation::State,
    offset: u64,
    length: u64,
    modified: Option<SystemTime>,
    source: Option<rollout_log::Stamp>,
    turn_id: Option<String>,
    activity: Option<ThreadActivity>,
    has_start: bool,
    pending_tools: HashSet<String>,
    pending_message: bool,
    questions: Questions,
    synchronous_questions: HashSet<String>,
    metrics_revision: Option<String>,
}

impl Cursor {
    fn apply(&mut self, record: Record) {
        let own = self.generation.owns(&record);
        self.generation.apply(&record);
        let payload = record.payload;
        let at = record
            .timestamp
            .as_deref()
            .and_then(super::diagnostics::timestamp);
        if record.kind == "token_usage_record" {
            if self.activity.is_some()
                && payload
                    .turn_id
                    .as_ref()
                    .is_none_or(|turn| self.turn_id.as_ref().is_none_or(|current| current == turn))
            {
                if let Some(response) = payload.response_id.filter(|id| !id.is_empty()) {
                    self.metrics_revision = Some(response);
                }
            }
            return;
        }
        if record.kind == "event_msg" && payload.kind == "item_completed" {
            if let Some(item) = &payload.item {
                self.questions.item(item, at);
            }
        }
        if record.kind == "event_msg" && payload.kind == "user_message" {
            if let Some(message) = payload.message.as_str() {
                self.questions
                    .user_message(&serde_json::json!([{"text":message}]), at);
            }
        }
        if record.kind == "response_item" {
            if payload.kind == "message" && payload.role.as_deref() == Some("user") {
                self.questions.user_message(&payload.content, at);
            }
            if payload.kind == "function_call"
                && matches!(
                    payload.name.as_deref(),
                    Some("request_user_input" | "request_user_input_async")
                )
            {
                if let (Some(call), Some(arguments)) = (&payload.call_id, &payload.arguments) {
                    if let Ok(value) = serde_json::from_str::<Value>(arguments) {
                        if let Some(questions) = value["questions"].as_array() {
                            self.questions.add(call, questions.len(), at);
                            if payload.name.as_deref() == Some("request_user_input") {
                                self.synchronous_questions.insert(call.clone());
                            }
                        }
                    }
                }
            }
            if payload.kind == "function_call_output" {
                if let Some(call) = &payload.call_id {
                    if self.synchronous_questions.remove(call) {
                        self.questions.resolve_call(call);
                    }
                }
            }
        }
        if record.kind == "compacted" {
            if own {
                if let Some(id) = &payload.compaction_response_id {
                    self.metrics_revision = Some(format!("compaction:{id}"));
                }
            }
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
                && content_has_text(&payload.content)
                || payload.kind == "reasoning"
                    && (content_has_text(&payload.content)
                        || payload.summary
                        || payload.encrypted_content)
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
                && (content_has_text(&payload.message) || payload.text);
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
        if working
            && self.activity.as_ref().is_some_and(|activity| {
                self.pending_message && started_at < activity.last_user_message_at
                    || !activity.working
                        && self.turn_id == payload.turn_id
                        && started_at <= activity.activity_at
            })
        {
            return;
        }
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
        if !working {
            for call in self.synchronous_questions.drain() {
                self.questions.resolve_call(&call);
            }
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
        if working {
            self.metrics_revision = None;
        }
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
            ..Default::default()
        });
    }

    fn read(&mut self, file: impl Read, end: u64) -> std::io::Result<()> {
        let mut reader = BufReader::new(file.take(end.saturating_sub(self.offset)));
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
            } else {
                self.generation.uncertain();
            }
        }
        Ok(())
    }
}

pub(super) fn read_rollout_snapshot(
    path: &Path,
    cutoff: Option<i64>,
    id: &str,
) -> Result<Option<ThreadActivity>, String> {
    static CACHE: OnceLock<Mutex<HashMap<(PathBuf, String), Cursor>>> = OnceLock::new();
    let mut log = match rollout_log::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.to_string()),
    };
    let source = log.stamp.clone();
    let modified = source.modified;
    let mut cache = CACHE
        .get_or_init(Default::default)
        .lock()
        .map_err(|error| error.to_string())?;
    let cursor = cache
        .entry((rollout_log::logical_path(path), id.into()))
        .or_default();
    if log.compressed {
        if cursor.source.as_ref() != Some(&source) {
            *cursor = Cursor::default();
            cursor.generation = generation::State::new(id);
            cursor
                .read(
                    log.reader_at(0).map_err(|error| error.to_string())?,
                    u64::MAX,
                )
                .map_err(|error| error.to_string())?;
        }
    } else {
        let length = log.len().map_err(|error| error.to_string())?;
        if cursor
            .source
            .as_ref()
            .is_some_and(|stamp| stamp.path != source.path)
            || cursor.modified.is_none()
            || length < cursor.length
            || length == cursor.length && modified != cursor.modified
        {
            // Bootstrap from the tail; grow only until the latest task start is found.
            let mut window = 65536;
            loop {
                *cursor = Cursor::default();
                cursor.generation = generation::State::new(id);
                let mut header = String::new();
                BufReader::new(
                    rollout_log::open(path)
                        .and_then(|log| log.reader_at(0))
                        .map_err(|e| e.to_string())?,
                )
                .read_line(&mut header)
                .map_err(|e| e.to_string())?;
                if let Ok(record) = serde_json::from_str::<Record>(&header) {
                    if record.kind == "session_meta" {
                        cursor.generation.apply(&record);
                    }
                }
                cursor.offset = length.saturating_sub(window);
                let file = rollout_log::open(path)
                    .and_then(|log| log.reader_at(cursor.offset))
                    .map_err(|error| error.to_string())?;
                let mut reader = BufReader::new(file);
                if cursor.offset > 0 {
                    let skipped = reader
                        .read_until(b'\n', &mut Vec::new())
                        .map_err(|error| error.to_string())?;
                    cursor.offset += skipped as u64;
                }
                cursor
                    .read(reader, length)
                    .map_err(|error| error.to_string())?;
                let old_terminal = cursor.activity.as_ref().is_some_and(|activity| {
                    !activity.working
                        && cutoff
                            .is_some_and(|cutoff| activity.activity_at < seconds_or_millis(cutoff))
                });
                if cursor.has_start || cursor.pending_message || old_terminal || window >= length {
                    break;
                }
                window = window.saturating_mul(2);
            }
        } else if length > cursor.length {
            cursor
                .read(
                    log.reader_at(cursor.offset)
                        .map_err(|error| error.to_string())?,
                    length,
                )
                .map_err(|error| error.to_string())?;
        }
        cursor.length = length;
    }
    cursor.modified = modified;
    cursor.source = Some(source);
    let mut activity = cursor.activity.clone();
    if let Some(activity) = activity.as_mut() {
        activity.questions = cursor.questions.clone();
        activity.metrics_revision = cursor.metrics_revision.clone();
        activity.observed = true;
        let mut generation = cursor.generation.value.clone();
        let turn = activity.turn_id.clone().unwrap_or_default();
        if generation.turn_id != turn {
            generation.config = Default::default();
        }
        generation.turn_id = turn;
        generation.phase = if activity.execution_status.as_deref() == Some("compacting") {
            "compacting"
        } else if activity.working {
            "running"
        } else {
            "idle"
        }
        .into();
        generation.at = Some(activity.last_response_at.max(activity.activity_at));
        activity.generation = Some(generation);
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
    Ok(activity)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs::File, io::Write};
    fn read_rollout_snapshot(
        path: &Path,
        cutoff: Option<i64>,
    ) -> Result<Option<ThreadActivity>, String> {
        super::read_rollout_snapshot(path, cutoff, "main")
    }

    #[test]
    fn generation_event_time_survives_compression_without_becoming_fresh() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("main.jsonl");
        std::fs::write(&path,concat!("{\"type\":\"session_meta\",\"payload\":{\"id\":\"main\"}}\n", "{\"type\":\"event_msg\",\"timestamp\":\"2026-10-28T00:00:01Z\",\"payload\":{\"type\":\"task_started\",\"turn_id\":\"one\"}}\n", "{\"type\":\"response_item\",\"timestamp\":\"2026-10-28T00:00:02Z\",\"payload\":{\"type\":\"reasoning\",\"summary\":[{\"text\":\"done\"}]}}\n")).unwrap();
        let first = read_rollout_snapshot(&path, Some(100))
            .unwrap()
            .unwrap()
            .generation
            .unwrap();
        let foreign = super::read_rollout_snapshot(&path, Some(100), "fork")
            .unwrap()
            .unwrap()
            .generation
            .unwrap();
        assert!(
            foreign.config.model.is_none(),
            "a shared physical path cannot reuse another thread's generation state"
        );
        crate::rollout_log::compress_fixture(&path);
        let migrated = read_rollout_snapshot(&path, Some(100))
            .unwrap()
            .unwrap()
            .generation
            .unwrap();
        assert_eq!(first.at, migrated.at);
        assert_eq!(first.phase, migrated.phase);
        assert_eq!(
            first.at,
            super::super::diagnostics::timestamp("2026-10-28T00:00:02Z")
        );
    }

    #[test]
    fn completed_compaction_refreshes_own_samples_without_using_child_events() {
        let mut cursor = Cursor {
            generation: generation::State::new("main"),
            ..Default::default()
        };
        for value in [
            serde_json::json!({"type":"session_meta","payload":{"id":"main"}}),
            serde_json::json!({"type":"event_msg","payload":{"type":"task_started","turn_id":"turn","started_at":120}}),
            serde_json::json!({"type":"token_usage_record","payload":{"turn_id":"turn","response_id":"compact-request"}}),
            serde_json::json!({"type":"compacted","payload":{"thread_id":"main","compaction_response_id":"compact-request"}}),
        ] {
            cursor.apply(serde_json::from_value(value).unwrap());
        }
        assert_eq!(
            cursor.metrics_revision.as_deref(),
            Some("compaction:compact-request")
        );
        cursor.apply(serde_json::from_value(serde_json::json!({"type":"compacted","payload":{"thread_id":"child","compaction_response_id":"foreign"}})).unwrap());
        assert_eq!(
            cursor.metrics_revision.as_deref(),
            Some("compaction:compact-request")
        );
        assert!(cursor.activity.as_ref().unwrap().working);
    }
    #[test]
    fn delayed_start_cannot_revive_a_terminal_turn_but_a_real_resume_can() {
        let mut cursor = Cursor::default();
        for payload in [
            serde_json::json!({"type":"task_started","turn_id":"turn","started_at":120}),
            serde_json::json!({"type":"task_complete","turn_id":"turn","started_at":120,"completed_at":130}),
            serde_json::json!({"type":"task_started","turn_id":"turn","started_at":120}),
        ] {
            cursor.apply(
                serde_json::from_value(serde_json::json!({"type":"event_msg","payload":payload}))
                    .unwrap(),
            );
        }
        assert!(!cursor.activity.as_ref().unwrap().working);
        cursor.apply(serde_json::from_value(serde_json::json!({"type":"event_msg","payload":{"type":"task_started","turn_id":"turn","started_at":140}})).unwrap());
        assert!(cursor.activity.as_ref().unwrap().working);
        assert_eq!(
            cursor.activity.as_ref().unwrap().execution_started_at,
            Some(140000)
        );
    }
    #[test]
    fn question_events_follow_answers_across_turns_and_compression() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("questions.jsonl");
        let mut events = vec![
            serde_json::json!({"type":"event_msg","payload":{"type":"task_started","turn_id":"first","started_at":120}}),
            serde_json::json!({"type":"response_item","payload":{"type":"function_call","name":"request_user_input_async","call_id":"q","arguments":"{\"questions\":[{\"title\":\"a\"},{\"title\":\"b\"}]}"}}),
            serde_json::json!({"type":"event_msg","payload":{"type":"item_completed","item":{"type":"AgentMessage","id":"q","questions":[{"title":"a"},{"title":"b"}]}}}),
            serde_json::json!({"type":"response_item","payload":{"type":"function_call_output","call_id":"q","output":"{\"accepted\":true}"}}),
            serde_json::json!({"type":"event_msg","payload":{"type":"task_complete","turn_id":"first","started_at":120,"completed_at":130}}),
        ];
        let write = |events: &[Value]| {
            std::fs::write(
                &path,
                events
                    .iter()
                    .map(|event| format!("{event}\n"))
                    .collect::<String>(),
            )
            .unwrap()
        };
        write(&events);
        let completed = read_rollout_snapshot(&path, Some(100)).unwrap().unwrap();
        assert!(
            completed.questions.is_pending(),
            "accepted is not an answer and completion does not close async questions"
        );
        assert!(!completed.working);
        events.push(serde_json::json!({"type":"event_msg","payload":{"type":"task_started","turn_id":"second","started_at":140}}));
        for index in 0..2 {
            let body = serde_json::json!([{"questionItemId":serde_json::json!(["request_user_input_async","q",index]).to_string(),"answer":"yes"}]);
            events.push(serde_json::json!({"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":format!("<send_user_message_question_reply>\n{body}\n</send_user_message_question_reply>")}]}}));
            write(&events);
            let active = read_rollout_snapshot(&path, Some(100)).unwrap().unwrap();
            assert!(active.working);
            assert_eq!(active.questions.is_pending(), index == 0);
        }
        crate::rollout_log::compress_fixture(&path);
        assert!(!read_rollout_snapshot(&path, Some(100))
            .unwrap()
            .unwrap()
            .questions
            .is_pending());
        let mut cursor = Cursor::default();
        for payload in [
            serde_json::json!({"type":"function_call","name":"request_user_input","call_id":"sync","arguments":"{\"questions\":[{}]}"}),
            serde_json::json!({"type":"function_call_output","call_id":"sync","output":"{\"answers\":{}}"}),
        ] {
            cursor.apply(
                serde_json::from_value(
                    serde_json::json!({"type":"response_item","payload":payload}),
                )
                .unwrap(),
            );
            assert_eq!(
                cursor.questions.is_pending(),
                !cursor.synchronous_questions.is_empty()
            );
        }
        assert!(!cursor.questions.is_pending());
    }

    #[test]
    fn ordinary_input_in_both_log_shapes_clears_questions_after_compression() {
        for (kind, message) in [
            (
                "event_msg",
                serde_json::json!({"type":"user_message","message":"先这样吧，明天我再试登录"}),
            ),
            (
                "response_item",
                serde_json::json!({"type":"message","role":"user","content":[{"type":"input_text","text":"先这样吧，明天我再试登录"}]}),
            ),
        ] {
            let directory = tempfile::tempdir().unwrap();
            let path = directory.path().join("questions.jsonl");
            let mut events = vec![
                serde_json::json!({"type":"event_msg","timestamp":"1970-01-01T00:02:00Z","payload":{"type":"task_started","turn_id":"turn"}}),
                serde_json::json!({"type":"response_item","timestamp":"1970-01-01T00:02:01Z","payload":{"type":"function_call","name":"request_user_input_async","call_id":"old","arguments":"{\"questions\":[{}]}"}}),
            ];
            std::fs::write(
                &path,
                events
                    .iter()
                    .map(|event| format!("{event}\n"))
                    .collect::<String>(),
            )
            .unwrap();
            let stale = read_rollout_snapshot(&path, Some(100)).unwrap().unwrap();
            assert!(stale.questions.is_pending());
            events.push(serde_json::json!({"type":kind,"timestamp":"1970-01-01T00:02:02Z","payload":message}));
            let mut file = File::options().append(true).open(&path).unwrap();
            writeln!(file, "{}", events.last().unwrap()).unwrap();
            drop(file);
            let mut current = read_rollout_snapshot(&path, Some(100)).unwrap().unwrap();
            current.questions.merge(&stale.questions);
            assert!(!current.questions.is_pending());
            assert!(
                current.working,
                "ordinary input does not invent an execution state"
            );
            crate::rollout_log::compress_fixture(&path);
            let mut replay = read_rollout_snapshot(&path, Some(100)).unwrap().unwrap();
            replay.questions.merge(&stale.questions);
            assert!(
                !replay.questions.is_pending(),
                "compression replay cannot restore the old pending question"
            );
        }
    }

    #[test]
    fn metrics_revision_follows_settlements_and_survives_compression() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("metrics.jsonl");
        std::fs::write(&path, concat!(
            "{\"type\":\"event_msg\",\"payload\":{\"type\":\"task_started\",\"turn_id\":\"main\",\"started_at\":120}}\n",
            "{\"type\":\"token_usage_record\",\"payload\":{\"turn_id\":\"main\",\"response_id\":\"response-one\"}}\n",
            "{\"type\":\"token_usage_record\",\"payload\":{\"turn_id\":\"other\",\"response_id\":\"foreign-response\"}}\n"
        )).unwrap();
        let read = || read_rollout_snapshot(&path, Some(100)).unwrap().unwrap();
        assert_eq!(read().metrics_revision.as_deref(), Some("response-one"));
        let mut file = File::options().append(true).open(&path).unwrap();
        write!(file, "{{\"type\":\"token_usage_record\",\"payload\":{{\"turn_id\":\"main\",\"response_id\":\"response-two\"}}}}").unwrap();
        assert_eq!(read().metrics_revision.as_deref(), Some("response-one"));
        writeln!(file).unwrap();
        assert_eq!(read().metrics_revision.as_deref(), Some("response-two"));
        writeln!(file, "{{\"type\":\"event_msg\",\"payload\":{{\"type\":\"turn_aborted\",\"turn_id\":\"main\",\"completed_at\":130}}}}").unwrap();
        drop(file);
        assert_eq!(read().metrics_revision.as_deref(), Some("response-two"));
        crate::rollout_log::compress_fixture(&path);
        assert_eq!(read().metrics_revision.as_deref(), Some("response-two"));
        std::fs::write(&path, "{\"type\":\"event_msg\",\"payload\":{\"type\":\"task_started\",\"turn_id\":\"next\",\"started_at\":140}}\n").unwrap();
        assert!(read().metrics_revision.is_none());
    }

    fn read_rollout_activity(
        path: &Path,
        cutoff: Option<i64>,
    ) -> Result<Option<ThreadActivity>, String> {
        read_rollout_snapshot(path, cutoff)
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
        crate::rollout_log::compress_fixture(&path);
        let compressed = read_rollout_activity(&path, Some(1788150000))
            .unwrap()
            .unwrap();
        assert_eq!(compressed.execution_ms, completed.execution_ms);
        assert_eq!(
            compressed.last_user_message_at,
            completed.last_user_message_at
        );
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
}
