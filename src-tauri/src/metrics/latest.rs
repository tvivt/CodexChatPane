use super::*;
use crate::rollout_log::{self, Stamp};
use std::{
    path::PathBuf,
    sync::{Mutex, OnceLock},
};

struct Entry {
    home: PathBuf,
    id: String,
    stamp: Stamp,
    dependencies: Vec<Stamp>,
    offset: u64,
    guard: Vec<u8>,
    parser: Parser,
    snapshot: ChatMetrics,
}

pub(super) fn read(home: &Path, id: &str) -> Result<ChatMetrics, String> {
    // Only the selected conversation needs a live cursor; details use the full reader.
    static CACHE: OnceLock<Mutex<Option<Entry>>> = OnceLock::new();
    let mut cache = CACHE
        .get_or_init(Default::default)
        .lock()
        .map_err(|e| e.to_string())?;
    read_cached(home, id, &mut cache)
}

fn read_cached(home: &Path, id: &str, cache: &mut Option<Entry>) -> Result<ChatMetrics, String> {
    let (path, data) = catalog(home, id)?;
    let log = match rollout_log::open(Path::new(&path)) {
        Ok(log) => log,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let segments = crate::preview::metrics_segments(home, id, Path::new(&path))?;
            rollout_log::open(&segments.last().ok_or("无法定位对话日志")?.0)
                .map_err(|e| e.to_string())?
        }
        Err(error) => return Err(error.to_string()),
    };
    let stamp = log.stamp.clone();
    let matching = cache
        .as_ref()
        .filter(|entry| entry.id == id && entry.home == home)
        .filter(|entry| {
            entry
                .dependencies
                .iter()
                .all(|stamp| rollout_log::open(&stamp.path).is_ok_and(|log| log.stamp == *stamp))
        });
    if let Some(entry) = matching.filter(|entry| entry.stamp == stamp) {
        let mut snapshot = entry.snapshot.clone();
        snapshot.as_of = data.as_of;
        return Ok(snapshot);
    }
    let append = matching.is_some_and(|entry| {
        !log.compressed
            && entry.stamp.path == stamp.path
            && stamp.len > entry.stamp.len
            && guard(&stamp.path, entry.offset).is_ok_and(|bytes| bytes == entry.guard)
    });
    let mut entry = if append {
        cache.take().unwrap()
    } else {
        let (start, end) = last_start(&stamp.path, None, id)?;
        let (segments, start) = if let Some(start) = start {
            (vec![(stamp.path.clone(), end)], start)
        } else {
            // A continuation can begin in the middle of a turn. Follow its history only
            // until the current turn's start, never turn inherited usage into own usage.
            let segments = crate::preview::metrics_segments(home, id, Path::new(&path))?;
            let mut selected = (0, 0);
            for (index, (path, end)) in segments.iter().enumerate().rev() {
                if let Some(start) = last_start(path, Some(*end), id)?.0 {
                    selected = (index, start);
                    break;
                }
            }
            (
                segments.into_iter().skip(selected.0).collect::<Vec<_>>(),
                selected.1,
            )
        };
        let mut parser = Parser::new(id);
        let mut dependencies = vec![];
        let mut offset = 0;
        for (index, (path, end)) in segments.iter().enumerate() {
            offset = feed(&mut parser, path, if index == 0 { start } else { 0 }, *end)?;
            if index + 1 < segments.len() {
                dependencies.push(rollout_log::open(path).map_err(|e| e.to_string())?.stamp);
            }
        }
        Entry {
            home: home.into(),
            id: id.into(),
            stamp: stamp.clone(),
            dependencies,
            offset,
            guard: vec![],
            parser,
            snapshot: data.clone(),
        }
    };
    if append {
        entry.offset = feed(&mut entry.parser, &stamp.path, entry.offset, stamp.len)?;
    }
    entry.stamp = stamp;
    entry.guard = if log.compressed {
        vec![]
    } else {
        guard(&entry.stamp.path, entry.offset)?
    };
    entry.snapshot = snapshot(data, &entry.parser);
    let result = entry.snapshot.clone();
    *cache = Some(entry);
    Ok(result)
}

fn guard(path: &Path, offset: u64) -> Result<Vec<u8>, String> {
    let start = offset.saturating_sub(128);
    let mut bytes = vec![];
    rollout_log::open(path)
        .and_then(|log| log.reader_at(start))
        .and_then(|reader| reader.take(offset - start).read_to_end(&mut bytes))
        .map_err(|e| e.to_string())?;
    Ok(bytes)
}

fn own_start(line: &str, id: &str, active: &mut String) -> bool {
    if !line.contains("task_started") && !line.contains("session_meta") && !line.contains(id) {
        return false;
    }
    let Ok(record) = serde_json::from_str::<Value>(line) else {
        return false;
    };
    let p = &record["payload"];
    if record["type"] == "session_meta" {
        *active = text(&p["id"]).into();
        return false;
    }
    let explicit = p["thread_id"].as_str();
    if explicit == Some(id) {
        *active = id.into();
    }
    record["type"] == "event_msg"
        && p["type"] == "task_started"
        && p["turn_id"].as_str().is_some()
        && (explicit == Some(id) || explicit.is_none() && active == id)
}

fn last_start(path: &Path, limit: Option<u64>, id: &str) -> Result<(Option<u64>, u64), String> {
    let log = rollout_log::open(path).map_err(|e| e.to_string())?;
    let mut header = String::new();
    BufReader::new(
        rollout_log::open(path)
            .and_then(|log| log.reader_at(0))
            .map_err(|e| e.to_string())?,
    )
    .read_line(&mut header)
    .map_err(|e| e.to_string())?;
    let mut owner = id.to_owned();
    own_start(&header, id, &mut owner);
    if !log.compressed && owner == id {
        let end = limit.unwrap_or(log.stamp.len).min(log.stamp.len);
        let mut window = 64 * 1024u64;
        loop {
            let start = end.saturating_sub(window);
            let mut bytes = vec![];
            rollout_log::open(path)
                .and_then(|log| log.reader_at(start))
                .and_then(|reader| reader.take(end - start).read_to_end(&mut bytes))
                .map_err(|e| e.to_string())?;
            let skip = if start == 0 {
                0
            } else {
                bytes
                    .iter()
                    .position(|b| *b == b'\n')
                    .map_or(bytes.len(), |n| n + 1)
            };
            let mut offset = start + skip as u64;
            let mut found = None;
            let mut active = owner.clone();
            for line in bytes[skip..].split_inclusive(|b| *b == b'\n') {
                if line.ends_with(b"\n")
                    && std::str::from_utf8(line).is_ok_and(|line| own_start(line, id, &mut active))
                {
                    found = Some(offset);
                }
                offset += line.len() as u64;
            }
            if found.is_some() || start == 0 {
                return Ok((found, end));
            }
            window = window.saturating_mul(2);
        }
    }
    let mut reader = BufReader::new(
        log.reader_at(0)
            .map_err(|e| e.to_string())?
            .take(limit.unwrap_or(u64::MAX)),
    );
    let mut line = String::new();
    let (mut offset, mut found) = (0, None);
    loop {
        line.clear();
        let len = reader.read_line(&mut line).map_err(|e| e.to_string())?;
        if len == 0 || !line.ends_with('\n') {
            break;
        }
        if own_start(&line, id, &mut owner) {
            found = Some(offset);
        }
        offset += len as u64;
    }
    Ok((found, offset))
}

fn feed(parser: &mut Parser, path: &Path, start: u64, end: u64) -> Result<u64, String> {
    let mut reader = BufReader::new(
        rollout_log::open(path)
            .and_then(|log| log.reader_at(start))
            .map_err(|e| e.to_string())?
            .take(end.saturating_sub(start)),
    );
    let mut offset = start;
    let mut line = String::new();
    loop {
        line.clear();
        let len = reader.read_line(&mut line).map_err(|e| e.to_string())?;
        // Leave partial records for the next append; they must not spoil a settled sample.
        if len == 0 || !line.ends_with('\n') {
            break;
        }
        offset += len as u64;
        match serde_json::from_str::<Value>(&line) {
            Ok(record) => {
                let p = &record["payload"];
                let own = p["thread_id"]
                    .as_str()
                    .map_or(parser.active_thread == parser.id, |id| id == parser.id);
                let boundary = record["type"] == "turn_context"
                    || record["type"] == "event_msg" && p["type"] == "task_started";
                let turn = p["turn_id"].as_str().or_else(|| {
                    p["internal_chat_message_metadata_passthrough"]["turn_id"].as_str()
                });
                if own
                    && !boundary
                    && turn.is_some_and(|turn| {
                        parser.order.last().is_some_and(|current| current != turn)
                    })
                {
                    continue;
                }
                if own && boundary {
                    if let Some(turn) = turn {
                        if parser.order.last().is_some_and(|old| old != turn) {
                            *parser = Parser::new(&parser.id);
                        }
                    }
                }
                parser.apply(record);
            }
            Err(_) => parser.bad_line(),
        }
    }
    Ok(offset)
}

fn snapshot(mut data: ChatMetrics, parser: &Parser) -> ChatMetrics {
    let (mut turns, _, _, warnings, complete) = parser.clone().finish();
    data.complete = complete;
    data.warnings = warnings;
    if let Some(mut turn) = turns.pop() {
        data.latest_turn_id = Some(turn.turn_id.clone());
        data.effort = turn.effort.clone();
        let selected = turn.requests.iter().rposition(|request| {
            request.included
                && turn
                    .started_at
                    .is_none_or(|start| request.start_ms.is_some_and(|at| at >= start))
        });
        let mut metrics = selected
            .map(|index| summarize(&turn.requests[index..=index]))
            .unwrap_or_default();
        // Native turn TTFT describes the first request, never a later tool continuation.
        if selected == Some(0) {
            metrics.ttft_ms = turn.metrics.ttft_ms;
            metrics.ttft_median_ms = turn.metrics.ttft_median_ms;
            metrics.ttft_sample_count = turn.metrics.ttft_sample_count;
            metrics.ttft_estimated = turn.metrics.ttft_estimated;
        }
        turn.requests = selected
            .map(|index| vec![turn.requests[index].clone()])
            .unwrap_or_default();
        turn.activities.clear();
        turn.metrics = metrics.clone();
        data.summary = metrics;
        data.turns = vec![turn];
    }
    data
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::io::Write;

    fn fixture(records: &[Value]) -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("sessions")).unwrap();
        let path = dir.path().join("sessions/rollout-main_fragment.jsonl");
        std::fs::write(&path, lines(records)).unwrap();
        let db = Connection::open(dir.path().join("state_5.sqlite")).unwrap();
        db.execute_batch("CREATE TABLE threads(id TEXT,rollout_path TEXT,title TEXT,cwd TEXT,model TEXT,archived INTEGER,created_at INTEGER,updated_at INTEGER)").unwrap();
        db.execute(
            "INSERT INTO threads VALUES('main',?1,'test',NULL,'test-model',0,1,2)",
            [path.to_str().unwrap()],
        )
        .unwrap();
        (dir, path)
    }
    fn lines(records: &[Value]) -> String {
        records.iter().map(|record| format!("{record}\n")).collect()
    }
    fn started(turn: &str) -> Value {
        json!({"type":"event_msg","timestamp":"1970-01-01T00:00:01.100Z","payload":{"type":"task_started","turn_id":turn}})
    }
    fn request(turn: &str, id: &str, start: i64, end: i64, output: u64) -> Vec<Value> {
        vec![
            json!({"type":"event_msg","payload":{"type":"item_completed","thread_id":"main","turn_id":turn,"started_at_ms":start,"completed_at_ms":end,"item":{"type":"AgentMessage","id":id}}}),
            json!({"type":"token_usage_record","payload":{"thread_id":"main","turn_id":turn,"response_id":id,"usage":{"input_tokens":100,"cached_input_tokens":90,"output_tokens":output,"reasoning_output_tokens":0,"total_tokens":100+output}}}),
        ]
    }
    fn append(path: &Path, records: &[Value]) {
        write!(
            std::fs::File::options().append(true).open(path).unwrap(),
            "{}",
            lines(records)
        )
        .unwrap();
    }

    #[test]
    fn latest_valid_request_follows_appends_interruptions_and_new_turns() {
        let mut records = vec![
            json!({"type":"session_meta","payload":{"id":"main"}}),
            started("old"),
        ];
        records.extend(request("old", "history", 1200, 1300, 1001));
        records.push(started("turn"));
        records.extend(request("turn", "first", 2000, 3000, 31));
        let (dir, path) = fixture(&records);
        let mut cache = None;
        let first = read_cached(dir.path(), "main", &mut cache).unwrap();
        assert_eq!(first.turns.len(), 1);
        assert_eq!(first.summary.token_speed, Some(30.0));
        assert_eq!(first.summary.ttft_ms, Some(900.0));
        append(&path, &request("turn", "latest", 9000, 10000, 101));
        let mut foreign = request("turn", "child", 11000, 12000, 9001);
        for record in &mut foreign {
            record["payload"]["thread_id"] = json!("child");
        }
        append(&path, &foreign);
        // A usage record without output timing is not a substitute for a valid sample.
        append(&path, &request("turn", "invalid", 0, 0, 301));
        append(
            &path,
            &[
                json!({"type":"event_msg","payload":{"type":"turn_aborted","turn_id":"turn","time_to_first_token_ms":800}}),
            ],
        );
        let latest = read_cached(dir.path(), "main", &mut cache).unwrap();
        assert_eq!(latest.summary.token_speed, Some(100.0));
        assert_eq!(latest.summary.output_tokens, Some(101));
        assert_eq!(latest.summary.cache_hit_rate, Some(0.9));
        assert_eq!(
            latest.summary.ttft_ms, None,
            "turn TTFT cannot label a later request"
        );
        assert_eq!(latest.turns[0].requests[0].response_id, "latest");
        assert_eq!(latest.turns[0].status, "interrupted");
        assert!(latest.subagents.is_empty() && latest.tools.is_empty());
        assert_eq!(cache.as_ref().unwrap().parser.order, vec!["turn"]);
        append(
            &path,
            &request("old", "late-old-response", 11000, 12000, 901),
        );
        append(
            &path,
            &[json!({"type":"event_msg","payload":{"type":"task_complete","turn_id":"old"}})],
        );
        let late_old = read_cached(dir.path(), "main", &mut cache).unwrap();
        assert_eq!(late_old.latest_turn_id.as_deref(), Some("turn"));
        assert_eq!(late_old.turns[0].status, "interrupted");
        assert_eq!(late_old.summary.output_tokens, Some(101));
        let offset = cache.as_ref().unwrap().offset;
        write!(
            std::fs::File::options().append(true).open(&path).unwrap(),
            "{{\"type\":"
        )
        .unwrap();
        let partial = read_cached(dir.path(), "main", &mut cache).unwrap();
        assert_eq!(partial.summary.token_speed, Some(100.0));
        assert_eq!(cache.as_ref().unwrap().offset, offset);
        writeln!(
            std::fs::File::options().append(true).open(&path).unwrap(),
            "\"ignored\"}}"
        )
        .unwrap();
        append(&path, &[started("new-without-samples")]);
        assert_eq!(
            read_cached(dir.path(), "main", &mut cache)
                .unwrap()
                .summary
                .token_speed,
            None
        );
        // Replacing the rollout must invalidate the old request even if it was cached.
        std::fs::write(&path, lines(&[started("next")])).unwrap();
        let next = read_cached(dir.path(), "main", &mut cache).unwrap();
        assert_eq!(next.latest_turn_id.as_deref(), Some("next"));
        assert_eq!(next.summary.token_speed, None);
        append(&path, &request("next", "new", 20000, 21000, 51));
        assert_eq!(
            read_cached(dir.path(), "main", &mut cache)
                .unwrap()
                .summary
                .token_speed,
            Some(50.0)
        );
        crate::rollout_log::compress_fixture(&path);
        let compressed = read_cached(dir.path(), "main", &mut cache).unwrap();
        assert_eq!(compressed.summary.token_speed, Some(50.0));
        assert_eq!(
            read_cached(dir.path(), "main", &mut cache)
                .unwrap()
                .summary
                .token_speed,
            Some(50.0)
        );
    }

    #[test]
    fn continuation_keeps_tool_wait_and_inherited_usage_out() {
        let mut parent = vec![
            json!({"type":"session_meta","payload":{"id":"parent"}}),
            started("inherited"),
        ];
        parent.extend(request("inherited", "inherited-response", 1200, 1300, 9001));
        parent.push(json!({"type":"event_msg","payload":{"type":"thread_settings_applied","thread_id":"main"}}));
        parent.push(started("turn"));
        parent.extend(request("turn", "first", 2000, 3000, 31));
        parent.push(json!({"type":"response_item","timestamp":"1970-01-01T00:00:04Z","payload":{"type":"function_call","call_id":"tool","name":"tool"}}));
        parent.push(json!({"type":"token_usage_record","payload":{"thread_id":"main","turn_id":"turn","response_id":"tool-request","usage":{"input_tokens":100,"cached_input_tokens":90,"output_tokens":10,"reasoning_output_tokens":0,"total_tokens":110}}}));
        let (dir, path) = fixture(&parent);
        let end = std::fs::metadata(&path).unwrap().len();
        crate::rollout_log::compress_fixture(&path);
        let current = dir.path().join("sessions/rollout-main_next.jsonl");
        let mut records = vec![
            json!({"type":"session_meta","payload":{"id":"main","history_base":{"thread_id":"fragment","end_byte_offset":end,"end_ordinal_exclusive":100}}}),
            json!({"type":"turn_context","payload":{"turn_id":"turn"}}),
            json!({"type":"response_item","timestamp":"1970-01-01T00:00:08Z","payload":{"type":"function_call_output","turn_id":"turn","call_id":"tool"}}),
        ];
        records.extend(request("turn", "after-tool", 9000, 10000, 101));
        std::fs::write(&current, lines(&records)).unwrap();
        Connection::open(dir.path().join("state_5.sqlite"))
            .unwrap()
            .execute(
                "UPDATE threads SET rollout_path=?1",
                [current.to_str().unwrap()],
            )
            .unwrap();
        let mut cache = None;
        let latest = read_cached(dir.path(), "main", &mut cache).unwrap();
        assert_eq!(latest.summary.output_tokens, Some(101));
        assert_eq!(latest.summary.token_speed, Some(100.0));
        assert_eq!(latest.summary.ttft_ms, None);
        assert_eq!(latest.turns.len(), 1);
        assert_eq!(latest.turns[0].turn_id, "turn");
        assert_eq!(
            cache.as_ref().unwrap().parser.turns["turn"]
                .turn
                .requests
                .len(),
            3
        );
        assert!(cache.as_ref().unwrap().parser.calls["tool"].end.is_some());
        // A new turn appended to a continuation no longer needs old turn statistics.
        append(&current, &[started("next")]);
        let next = read_cached(dir.path(), "main", &mut cache).unwrap();
        assert_eq!(next.summary.token_speed, None);
        assert_eq!(cache.as_ref().unwrap().parser.order, vec!["next"]);
    }

    #[test]
    fn larger_replacement_is_not_treated_as_an_append() {
        let mut records = vec![started("old")];
        records.extend(request("old", "old-response", 2000, 3000, 31));
        let (dir, path) = fixture(&records);
        let mut cache = None;
        read_cached(dir.path(), "main", &mut cache).unwrap();
        let mut replacement = vec![started("replacement")];
        replacement.extend(request("replacement", "fresh", 4000, 5000, 201));
        replacement.push(json!({"type":"response_item","payload":{"type":"message","content":[{"text":"task_started 归档对话 ".repeat(5000)}]}}));
        std::fs::write(&path, lines(&replacement)).unwrap();
        let latest = read_cached(dir.path(), "main", &mut cache).unwrap();
        assert_eq!(latest.latest_turn_id.as_deref(), Some("replacement"));
        assert_eq!(latest.summary.token_speed, Some(200.0));
        assert_eq!(latest.turns[0].requests[0].response_id, "fresh");
        assert_eq!(cache.as_ref().unwrap().parser.order, vec!["replacement"]);
    }

    #[test]
    #[ignore = "reads local metrics; requires CODEX_PANE_VERIFY_METRICS_THREAD"]
    fn local_latest_request_matches_full_history() {
        let id = std::env::var("CODEX_PANE_VERIFY_METRICS_THREAD").unwrap();
        let home = crate::source::codex_home().unwrap();
        let latest = read(&home, &id).unwrap();
        let full = read_at(&home, &id, false).unwrap();
        assert!(latest.turns.len() <= 1 && latest.subagents.is_empty());
        if let Some(turn) = latest.turns.first() {
            let full_turn = full
                .turns
                .iter()
                .find(|t| t.turn_id == turn.turn_id)
                .unwrap();
            if let Some(request) = turn.requests.first() {
                let (index, expected) = full_turn
                    .requests
                    .iter()
                    .enumerate()
                    .find(|(_, r)| r.response_id == request.response_id)
                    .unwrap();
                assert!(expected.included);
                let metrics = summarize(std::slice::from_ref(expected));
                assert_eq!(latest.summary.token_speed, metrics.token_speed);
                assert_eq!(latest.summary.input_tokens, metrics.input_tokens);
                assert_eq!(latest.summary.cache_hit_rate, metrics.cache_hit_rate);
                if index != 0 {
                    assert_eq!(latest.summary.ttft_ms, None);
                }
                println!(
                    "turn={} response={} metrics={}",
                    turn.turn_id,
                    request.response_id,
                    serde_json::to_string(&latest.summary).unwrap()
                );
            } else {
                assert_eq!(latest.summary.token_speed, None);
            }
        }
    }
}
