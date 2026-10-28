use super::*;
use crate::{
    rollout_log::{self, Stamp},
    usage::{context, costs::rounds, ledger::logs},
};
use rusqlite::{params, Connection, OptionalExtension};
use std::path::PathBuf;

const VERSION: &str = "usage-analysis-v1";

#[derive(Clone, Serialize, Deserialize)]
struct Segment {
    path: PathBuf,
    end: u64,
    stamp: Stamp,
}

#[derive(Serialize, Deserialize)]
struct Sources {
    version: String,
    segments: Vec<Segment>,
    offset: u64,
    header_len: u64,
    header_hash: u64,
    tail_hash: u64,
}

// Only parser state, validated usage and compact question/quota metadata; never message/tool bodies.
#[derive(Serialize, Deserialize)]
struct Cursor {
    version: String,
    segments: Vec<Segment>,
    offset: u64,
    header_len: u64,
    header_hash: u64,
    tail_hash: u64,
    pending_tail: bool,
    bad_records: bool,
    parent: Option<String>,
    parser: Parser,
    legacy: legacy::Collector,
    questions: rounds::Parser,
    quota: logs::Collector,
}

// Deserialize a small envelope first. Unknown fields are skipped without allocating their large bodies.
#[derive(Default, Serialize, Deserialize)]
struct Head {
    #[serde(rename = "type", default)]
    kind: String,
    timestamp: Option<Value>,
    #[serde(default)]
    payload: Payload,
}
#[derive(Default, Serialize, Deserialize)]
struct Payload {
    #[serde(rename = "type")]
    kind: Option<String>,
    role: Option<String>,
    phase: Option<String>,
    channel: Option<String>,
    id: Option<String>,
    thread_id: Option<String>,
    turn_id: Option<String>,
    root_turn_id: Option<String>,
    started_at: Option<Value>,
    completed_at: Option<Value>,
    duration_ms: Option<Value>,
    time_to_first_token_ms: Option<Value>,
    internal_chat_message_metadata_passthrough: Option<TurnId>,
    turn_attribution: Option<Attribution>,
    item: Option<Item>,
}
#[derive(Serialize, Deserialize)]
struct TurnId {
    turn_id: Option<String>,
}
#[derive(Serialize, Deserialize)]
struct Attribution {
    turn_trigger: Option<String>,
    parent_turn_id: Option<String>,
    root_turn_id: Option<String>,
}
#[derive(Serialize, Deserialize)]
struct Item {
    #[serde(rename = "type")]
    kind: Option<String>,
    phase: Option<String>,
    id: Option<String>,
}

fn record(line: &str) -> Result<Option<Value>, serde_json::Error> {
    let head = match serde_json::from_str::<Head>(line) {
        Ok(head) => head,
        Err(_) => return serde_json::from_str(line).map(Some),
    };
    let event = head.payload.kind.as_deref().unwrap_or("");
    let full = matches!(
        head.kind.as_str(),
        "session_meta" | "turn_context" | "token_usage_record"
    ) || head.kind == "event_msg"
        && matches!(
            event,
            "token_count" | "thread_settings_applied" | "user_message"
        )
        || head.kind == "response_item" && head.payload.role.as_deref() == Some("user")
        || head.payload.item.as_ref().and_then(|i| i.kind.as_deref()) == Some("UserMessage");
    if full {
        return serde_json::from_str(line).map(Some);
    }
    let boundary = matches!(event, "task_started" | "task_complete" | "turn_aborted")
        || head
            .payload
            .phase
            .as_deref()
            .or(head.payload.channel.as_deref())
            == Some("final")
        || head
            .payload
            .item
            .as_ref()
            .and_then(|item| item.phase.as_deref())
            == Some("final");
    if boundary {
        return serde_json::to_value(head).map(Some);
    }
    if head.payload.thread_id.is_none() && head.payload.turn_id.is_none() {
        return Ok(None);
    }
    Ok(Some(
        serde_json::json!({"payload":{"thread_id":head.payload.thread_id,"turn_id":head.payload.turn_id}}),
    ))
}

fn hash(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf29ce484222325, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
    })
}
fn guard(path: &Path, end: u64, len: u64) -> Result<u64, String> {
    let mut bytes = vec![];
    rollout_log::open(path)
        .and_then(|log| log.reader_at(end - len))
        .and_then(|reader| reader.take(len).read_to_end(&mut bytes))
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 != len {
        return Err("日志检查点已失效".into());
    }
    Ok(hash(&bytes))
}

fn database(db: &Connection) -> Result<(), String> {
    db.execute_batch("CREATE TABLE IF NOT EXISTS usage_log_analysis(thread_id TEXT PRIMARY KEY,data TEXT NOT NULL); CREATE TABLE IF NOT EXISTS usage_log_sources(thread_id TEXT PRIMARY KEY,data TEXT NOT NULL);").map_err(|e|e.to_string())
}

pub(crate) fn cached_signature(
    home: &Path,
    id: &str,
    path: &Path,
) -> Option<crate::usage::costs::Signature> {
    let db = context::database(home).ok()?;
    database(&db).ok()?;
    let data: String = db
        .query_row(
            "SELECT data FROM usage_log_sources WHERE thread_id=?1",
            [id],
            |r| r.get(0),
        )
        .ok()?;
    let mut source: Sources = serde_json::from_str(&data).ok()?;
    if source.version != VERSION {
        return None;
    }
    let current = rollout_log::open(path).ok()?;
    let last = source.segments.last()?;
    if rollout_log::logical_path(&last.path) != rollout_log::logical_path(&current.stamp.path)
        || !source
            .segments
            .iter()
            .take(source.segments.len() - 1)
            .all(|s| rollout_log::open(&s.path).is_ok_and(|log| log.stamp == s.stamp))
    {
        return None;
    }
    if last.stamp != current.stamp {
        if current.compressed
            || last.stamp.path != current.stamp.path
            || last.stamp.created != current.stamp.created
            || current.stamp.len <= last.stamp.len
            || guard(&current.stamp.path, source.header_len, source.header_len).ok()
                != Some(source.header_hash)
            || guard(&current.stamp.path, source.offset, source.offset.min(128)).ok()
                != Some(source.tail_hash)
        {
            return None;
        }
        source.segments.last_mut()?.stamp = current.stamp;
    }
    Some(
        source
            .segments
            .iter()
            .map(|s| {
                (
                    s.path.to_string_lossy().to_string(),
                    format!("usage-v3:{:?}", s.stamp),
                )
            })
            .collect(),
    )
}
fn cached(db: &Connection, id: &str) -> Result<Option<Cursor>, String> {
    database(db)?;
    let value: Option<String> = db
        .query_row(
            "SELECT data FROM usage_log_analysis WHERE thread_id=?1",
            [id],
            |r| r.get(0),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    Ok(value
        .and_then(|value| serde_json::from_str::<Cursor>(&value).ok())
        .filter(|c| c.version == VERSION))
}

pub(crate) fn read(home: &Path, id: &str) -> Result<(ChatMetrics, rounds::Details), String> {
    let Some(db) = context::database(home).ok() else {
        return read_in(home, id, None);
    };
    let identity = context::identity(home)?;
    let (path, _) = catalog(home, id)?;
    let stamp = rollout_log::open(Path::new(&path))
        .ok()
        .map(|log| log.stamp);
    context::share(format!("{VERSION}|{identity}|{id}|{stamp:?}"), || {
        load(home, id, Some(&db)).map(|_| ())
    })?;
    if context::identity(home)? != identity {
        return Err("账号或日志来源已切换，请重新读取".into());
    }
    let cursor = cached(&db, id)?.ok_or("用量分析缓存未完成")?;
    finish(home, id, cursor)
}

pub(super) fn read_in(
    home: &Path,
    id: &str,
    db: Option<&Connection>,
) -> Result<(ChatMetrics, rounds::Details), String> {
    finish(home, id, load(home, id, db)?)
}

fn load(home: &Path, id: &str, db: Option<&Connection>) -> Result<Cursor, String> {
    let (path, _) = catalog(home, id)?;
    let mut old = match db {
        Some(db) => cached(db, id)?,
        None => None,
    };
    let latest = rollout_log::open(Path::new(&path)).ok();
    let same_root = old
        .as_ref()
        .zip(latest.as_ref())
        .is_some_and(|(cursor, log)| {
            cursor.segments.last().is_some_and(|s| {
                rollout_log::logical_path(&s.path) == rollout_log::logical_path(&log.stamp.path)
            })
        });
    let dependencies = old.as_ref().is_some_and(|c| {
        c.segments
            .iter()
            .take(c.segments.len().saturating_sub(1))
            .all(|s| rollout_log::open(&s.path).is_ok_and(|log| log.stamp == s.stamp))
    });
    if same_root
        && dependencies
        && old
            .as_ref()
            .zip(latest.as_ref())
            .is_some_and(|(c, l)| c.segments.last().is_some_and(|s| s.stamp == l.stamp))
    {
        return Ok(old.take().unwrap());
    }
    let append = same_root
        && dependencies
        && old.as_ref().zip(latest.as_ref()).is_some_and(|(c, l)| {
            let previous = &c.segments.last().unwrap().stamp;
            !l.compressed
                && previous.path == l.stamp.path
                && previous.created == l.stamp.created
                && l.stamp.len > previous.len
                && c.offset <= l.stamp.len
                && guard(&l.stamp.path, c.header_len, c.header_len)
                    .is_ok_and(|value| value == c.header_hash)
                && guard(&l.stamp.path, c.offset, c.offset.min(128))
                    .is_ok_and(|value| value == c.tail_hash)
        });
    let mut cursor = if append {
        old.take().unwrap()
    } else {
        let segments = crate::preview::metrics_segments(home, id, Path::new(&path))?
            .into_iter()
            .map(|(path, end)| {
                let stamp = rollout_log::open(&path).map_err(|e| e.to_string())?.stamp;
                Ok(Segment { path, end, stamp })
            })
            .collect::<Result<Vec<_>, String>>()?;
        Cursor {
            version: VERSION.into(),
            segments,
            offset: 0,
            header_len: 0,
            header_hash: 0,
            tail_hash: 0,
            pending_tail: false,
            bad_records: false,
            parent: None,
            parser: Parser::new(id),
            legacy: legacy::Collector::new(id),
            questions: rounds::Parser::default(),
            quota: logs::Collector::default(),
        }
    };
    if append {
        let last = cursor.segments.last_mut().unwrap();
        last.stamp = latest.unwrap().stamp;
        last.end = last.stamp.len;
    }
    let first = if append { cursor.segments.len() - 1 } else { 0 };
    cursor.pending_tail = false;
    for index in first..cursor.segments.len() {
        let segment = &cursor.segments[index];
        let start = if append { cursor.offset } else { 0 };
        let mut reader = BufReader::new(
            rollout_log::open(&segment.path)
                .and_then(|log| log.reader_at(start))
                .map_err(|e| e.to_string())?
                .take(segment.end.saturating_sub(start)),
        );
        let mut offset = start;
        let mut line = String::new();
        loop {
            line.clear();
            let len = reader.read_line(&mut line).map_err(|e| e.to_string())?;
            if len == 0 {
                break;
            }
            if !line.ends_with('\n') {
                if segment.end == u64::MAX {
                    rollout_log::remember_length(&segment.stamp, offset + len as u64);
                }
                cursor.pending_tail = true;
                break;
            }
            if index + 1 == cursor.segments.len() && offset == 0 {
                cursor.header_len = len as u64;
                cursor.header_hash = hash(line.as_bytes());
            }
            offset += len as u64;
            let value = match record(&line) {
                Ok(Some(value)) => value,
                Ok(None) => continue,
                Err(_) => {
                    cursor.bad_records = true;
                    cursor.parser.bad_line();
                    continue;
                }
            };
            if value["type"] == "session_meta" && value["payload"]["id"] == id {
                cursor.parent = value["payload"]["parent_thread_id"]
                    .as_str()
                    .or_else(|| {
                        value["payload"]["source"]["subagent"]["thread_spawn"]["parent_thread_id"]
                            .as_str()
                    })
                    .map(str::to_owned);
            }
            cursor.legacy.apply(&value);
            cursor.questions.apply(id, &value);
            cursor.quota.apply(id, &value);
            if matches!(
                value["type"].as_str(),
                Some("session_meta" | "turn_context" | "token_usage_record")
            ) || value["type"] == "event_msg"
                && matches!(
                    value["payload"]["type"].as_str(),
                    Some(
                        "task_started"
                            | "task_complete"
                            | "turn_aborted"
                            | "thread_settings_applied"
                    )
                )
            {
                cursor.parser.apply(value);
            }
        }
        if segment.end == u64::MAX && !cursor.pending_tail {
            rollout_log::remember_length(&segment.stamp, offset);
        }
        if index + 1 == cursor.segments.len() {
            cursor.offset = offset;
        }
    }
    if !cursor
        .segments
        .iter()
        .all(|s| rollout_log::open(&s.path).is_ok_and(|log| log.stamp == s.stamp))
    {
        return Err("读取期间日志有更新，请重试".into());
    }
    let last = cursor.segments.last().unwrap();
    cursor.tail_hash = if last.stamp.path.extension().is_some_and(|ext| ext == "zst") {
        0
    } else {
        guard(&last.path, cursor.offset, cursor.offset.min(128))?
    };
    if let Some(db) = db {
        database(db)?;
        db.execute(
            "INSERT OR REPLACE INTO usage_log_analysis VALUES(?1,?2)",
            params![
                id,
                serde_json::to_string(&cursor).map_err(|e| e.to_string())?
            ],
        )
        .map_err(|e| e.to_string())?;
        let sources = Sources {
            version: VERSION.into(),
            segments: cursor.segments.clone(),
            offset: cursor.offset,
            header_len: cursor.header_len,
            header_hash: cursor.header_hash,
            tail_hash: cursor.tail_hash,
        };
        db.execute(
            "INSERT OR REPLACE INTO usage_log_sources VALUES(?1,?2)",
            params![
                id,
                serde_json::to_string(&sources).map_err(|e| e.to_string())?
            ],
        )
        .map_err(|e| e.to_string())?;
        if !cursor.pending_tail && !cursor.bad_records {
            let signature = cursor
                .segments
                .iter()
                .map(|s| {
                    (
                        s.path.to_string_lossy().to_string(),
                        format!("usage-v3:{:?}", s.stamp),
                    )
                })
                .collect::<Vec<_>>();
            // Quota discovery can reuse this necessary scan, but never starts Token analysis itself.
            if context::identity(home).is_ok() {
                logs::reuse(
                    home,
                    id,
                    &serde_json::to_string(&signature).map_err(|e| e.to_string())?,
                    &cursor.quota.rows,
                )?;
            }
        }
    }
    Ok(cursor)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    fn valid(
        data: &ChatMetrics,
    ) -> Vec<(String, Option<u64>, Option<u64>, Option<u64>, Option<u64>)> {
        data.turns
            .iter()
            .flat_map(|t| &t.requests)
            .filter(|r| {
                !matches!(
                    r.excluded_reason.as_deref(),
                    Some("用量冲突" | "用量字段无效")
                )
            })
            .map(|r| {
                (
                    r.response_id.clone(),
                    r.input_tokens,
                    r.cached_input_tokens,
                    r.output_tokens,
                    r.reasoning_tokens,
                )
            })
            .collect()
    }
    #[test]
    #[ignore = "measures selected real logs using a temporary analysis cache, never writes source logs"]
    fn live_analysis_audit() {
        let home = crate::source::codex_home().unwrap();
        let isolated = tempfile::tempdir().unwrap();
        context::confirm(isolated.path(), Some("benchmark")).unwrap();
        let db = context::database(isolated.path()).unwrap();
        let mode = std::env::var("CODEX_PANE_ANALYSIS_MODE").unwrap_or_default();
        for id in [
            "01a0ff60-165e-7ff1-89c5-4d58b94ce669",
            "01a162f3-1e03-7ff2-b959-44026ac0ff7b",
            "01a05327-af7d-7371-886d-7c0b06905ce4",
        ] {
            let expected = if mode != "optimized" {
                rollout_log::read_stats(true);
                let start = std::time::Instant::now();
                let mut data = read_at_mode(&home, id, false, true).unwrap();
                crate::usage::costs::attach_metrics(&mut data);
                println!(
                    "id={id} baseline_ms={} read={:?} digest={} requests={} input={:?} output={:?}",
                    start.elapsed().as_millis(),
                    rollout_log::read_stats(false),
                    hash(&serde_json::to_vec(&valid(&data)).unwrap()),
                    valid(&data).len(),
                    data.summary.input_tokens,
                    data.summary.output_tokens
                );
                Some(data)
            } else {
                None
            };
            if mode == "baseline" {
                continue;
            }
            for pass in 1..=2 {
                rollout_log::read_stats(true);
                let start = std::time::Instant::now();
                let (mut data, rounds) = read_in(&home, id, Some(&db)).unwrap();
                crate::usage::costs::attach_metrics(&mut data);
                if let Some(expected) = &expected {
                    assert_eq!(valid(&data), valid(expected));
                    assert_eq!(
                        serde_json::to_value(&data.summary).unwrap(),
                        serde_json::to_value(&expected.summary).unwrap()
                    );
                    assert_eq!(
                        serde_json::to_value(&data.consumption).unwrap(),
                        serde_json::to_value(&expected.consumption).unwrap()
                    );
                }
                let stats = rollout_log::read_stats(false);
                if pass == 2 {
                    assert_eq!(stats.bytes, 0);
                }
                let checkpoint: i64 = db
                    .query_row(
                        "SELECT length(data) FROM usage_log_analysis WHERE thread_id=?1",
                        [id],
                        |r| r.get(0),
                    )
                    .unwrap();
                println!("id={id} pass={pass} ms={} read={stats:?} digest={} requests={} rounds={} checkpoint_bytes={checkpoint}",start.elapsed().as_millis(),hash(&serde_json::to_vec(&valid(&data)).unwrap()),valid(&data).len(),rounds.rounds.len());
            }
        }
    }
    #[test]
    fn persisted_append_tail_replacement_and_compression_match_full_reader() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path();
        context::confirm(home, Some("test-account")).unwrap();
        let db = context::database(home).unwrap();
        let catalog = Connection::open(home.join("state_5.sqlite")).unwrap();
        let path = home.join("main.jsonl");
        catalog.execute_batch("CREATE TABLE threads(id TEXT,rollout_path TEXT,title TEXT,cwd TEXT,model TEXT,archived INTEGER,created_at INTEGER,updated_at INTEGER);").unwrap();
        catalog
            .execute(
                "INSERT INTO threads VALUES('main',?1,'Main','','gpt-6.1-sol',0,0,10)",
                [path.to_string_lossy().as_ref()],
            )
            .unwrap();
        catalog
            .execute_batch("ALTER TABLE threads ADD COLUMN thread_source TEXT DEFAULT 'user';")
            .unwrap();
        let request = |id: &str, input: u64| serde_json::json!({"type":"token_usage_record","timestamp":"1970-01-01T00:00:02Z","payload":{"turn_id":"one","response_id":id,"usage":{"input_tokens":input,"cached_input_tokens":input/2,"output_tokens":input/10,"reasoning_output_tokens":input/20}}});
        let mut text=[serde_json::json!({"type":"session_meta","payload":{"id":"main"}}),serde_json::json!({"type":"event_msg","payload":{"type":"task_started","turn_id":"one"}}),serde_json::json!({"type":"turn_context","payload":{"turn_id":"one","model":"gpt-6.1-sol"}}),serde_json::json!({"type":"event_msg","timestamp":"1970-01-01T00:00:00Z","payload":{"type":"user_message","message":"First question"}}),request("a",100)].iter().map(|v|format!("{v}\n")).collect::<String>();
        for _ in 0..70 {
            text.push_str(&format!("{}\n",serde_json::json!({"type":"response_item","payload":{"type":"function_call_output","output":"x".repeat(10_000)}})));
        }
        let legacy = |total: u64, last: u64| serde_json::json!({"type":"event_msg","timestamp":"1970-01-01T00:00:03Z","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":total,"cached_input_tokens":total/2,"output_tokens":total/10},"last_token_usage":{"input_tokens":last,"cached_input_tokens":last/2,"output_tokens":last/10}}}});
        text.push_str(&format!("{}\n", legacy(100, 100)));
        std::fs::write(&path, &text).unwrap();
        let check = || {
            let actual = read_in(home, "main", Some(&db)).unwrap().0;
            let expected = read_at_mode(home, "main", false, true).unwrap();
            assert_eq!(valid(&actual), valid(&expected));
            assert_eq!(
                serde_json::to_value(&actual.summary).unwrap(),
                serde_json::to_value(&expected.summary).unwrap()
            );
            actual
        };
        check();
        let initial: String = db
            .query_row(
                "SELECT data FROM usage_log_analysis WHERE thread_id='main'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(!initial.contains(&"x".repeat(100)));
        assert!(initial.len() < 100_000);
        crate::usage::costs::index(home, None, &Default::default(), |_, _| {}).unwrap();
        rollout_log::read_stats(true);
        read_in(home, "main", Some(&db)).unwrap();
        assert_eq!(
            rollout_log::read_stats(false).bytes,
            0,
            "unchanged analysis reads no source content"
        );
        let extra = format!("{}\n{}\n", request("b", 200), legacy(500, 400));
        std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap()
            .write_all(extra.as_bytes())
            .unwrap();
        rollout_log::read_stats(true);
        crate::usage::costs::index(home, None, &Default::default(), |_, _| {}).unwrap();
        assert!(
            rollout_log::read_stats(false).bytes < 20_000,
            "append reads only the suffix and small safety guards"
        );
        check();
        let partial = request("c", 300).to_string();
        std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap()
            .write_all(partial.as_bytes())
            .unwrap();
        let incomplete = read_in(home, "main", Some(&db)).unwrap().0;
        assert!(!incomplete.usage_complete);
        assert_eq!(valid(&incomplete).len(), 3);
        std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap()
            .write_all(b"\n")
            .unwrap();
        check();
        std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap()
            .write_all(format!("{}\n{}\n", request("a", 100), request("a", 110)).as_bytes())
            .unwrap();
        check();
        let rebuilt = format!(
            "{}\n{}\n",
            serde_json::json!({"type":"session_meta","payload":{"id":"main"}}),
            request("reset", 500)
        );
        std::fs::write(&path, &rebuilt).unwrap();
        assert_eq!(valid(&check()).len(), 1);
        std::fs::rename(&path, home.join("retired.jsonl")).unwrap();
        std::fs::write(
            path.with_extension("jsonl.zst"),
            zstd::stream::encode_all(rebuilt.as_bytes(), 1).unwrap(),
        )
        .unwrap();
        check();
        let saved: String = db
            .query_row(
                "SELECT data FROM usage_log_analysis WHERE thread_id='main'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(
            !saved.contains(&"x".repeat(100)),
            "body text never enters the checkpoint"
        );
    }
}

fn finish(
    home: &Path,
    id: &str,
    mut cursor: Cursor,
) -> Result<(ChatMetrics, rounds::Details), String> {
    let (_, mut data) = catalog(home, id)?;
    // Request.total_tokens is private in the wire format; its canonical value remains in usages.
    for (usage, turn, index) in cursor.parser.usages.values() {
        if let Some(request) = cursor
            .parser
            .turns
            .get_mut(turn)
            .and_then(|t| t.turn.requests.get_mut(*index))
        {
            request.total_tokens = usage.total_tokens;
        }
    }
    cursor.legacy.attach(&mut cursor.parser);
    if cursor.pending_tail {
        cursor.parser.bad_line();
    }
    let (turns, tools, _, warnings, complete) = cursor.parser.finish();
    data.summary = aggregate(&turns);
    data.effort = turns.last().and_then(|t| t.effort.clone());
    data.latest_turn_id = turns
        .iter()
        .max_by_key(|t| t.started_at.unwrap_or(0))
        .map(|t| t.turn_id.clone());
    data.turns = turns;
    data.tools = tools;
    data.warnings = warnings;
    data.complete = complete;
    data.usage_complete = !cursor.pending_tail && !cursor.bad_records;
    data.parent_thread_id = cursor.parent;
    cursor.questions.attach_requests(&data);
    Ok((data, cursor.questions.details))
}
