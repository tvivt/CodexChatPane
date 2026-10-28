use super::{database, signature, unchanged};
use crate::preview;
use rusqlite::{params, Connection, OpenFlags, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
};

#[derive(Clone, Default, Serialize, Deserialize)]
pub(crate) struct Details {
    pub requests: HashMap<String, usize>,
    pub rounds: Vec<Round>,
}

#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct Round {
    pub number: usize,
    pub prompt: String,
    pub started_at: Option<i64>,
    pub completed_at: Option<i64>,
    pub status: String,
    #[serde(default)]
    pub request_times: Vec<Option<i64>>,
}

#[derive(Clone, Default, Serialize, Deserialize)]
pub(crate) struct Parser {
    pub(crate) details: Details,
    active_thread: String,
    active_turn: Option<String>,
    turns: HashMap<String, usize>,
    aborted: HashSet<String>,
    current: Option<usize>,
    resume: HashMap<String, usize>,
}

impl Parser {
    fn current(&mut self, at: Option<i64>) -> usize {
        if let Some(index) = self
            .current
            .filter(|&i| self.details.rounds[i].status != "completed")
        {
            return index;
        }
        let index = self.details.rounds.len();
        self.details.rounds.push(Round {
            number: index + 1,
            prompt: String::new(),
            started_at: at,
            completed_at: None,
            status: "inProgress".into(),
            request_times: vec![],
        });
        self.current = Some(index);
        index
    }

    pub(crate) fn apply(&mut self, id: &str, record: &Value) {
        let p = &record["payload"];
        let kind = record["type"].as_str().unwrap_or("");
        let event = p["type"].as_str().unwrap_or("");
        if kind == "session_meta" {
            self.active_thread = p["id"].as_str().unwrap_or("").into();
            self.active_turn = None;
            return;
        }
        if p["thread_id"].as_str().is_some_and(|thread| thread != id) {
            return;
        }
        if p["thread_id"] == id {
            self.active_thread = id.into();
        }
        if self.active_thread != id {
            return;
        }
        let at = record["timestamp"].as_str().and_then(|s| {
            time::OffsetDateTime::parse(s, &time::format_description::well_known::Rfc3339)
                .ok()
                .map(|t| (t.unix_timestamp_nanos() / 1_000_000) as i64)
        });
        let turn = p["turn_id"]
            .as_str()
            .or_else(|| p["internal_chat_message_metadata_passthrough"]["turn_id"].as_str());
        if kind == "turn_context" || kind == "event_msg" && event == "task_started" {
            if let Some(turn) = turn {
                let index = if let Some(index) = self.turns.get(turn).copied() {
                    index
                } else {
                    let linked = p["turn_attribution"]["parent_turn_id"]
                        .as_str()
                        .or_else(|| p["root_turn_id"].as_str().filter(|root| *root != turn))
                        .and_then(|parent| self.turns.get(parent))
                        .copied();
                    let goal = p["turn_attribution"]["turn_trigger"] == "goal";
                    let previous = self.current;
                    if let Some(index) = linked.or_else(|| goal.then_some(previous).flatten()) {
                        self.current = Some(index);
                        self.details.rounds[index].status = "inProgress".into();
                        self.details.rounds[index].completed_at = None;
                        index
                    } else {
                        if self.active_turn.is_some()
                            && previous.is_some_and(|i| !self.details.rounds[i].prompt.is_empty())
                        {
                            self.current = None;
                        }
                        let index = self.current(at);
                        if let Some(old) =
                            previous.filter(|&i| self.details.rounds[i].status == "interrupted")
                        {
                            self.resume.insert(turn.into(), old);
                        }
                        index
                    }
                };
                self.turns.insert(turn.into(), index);
                self.active_turn = Some(turn.into());
                if self.details.rounds[index].status == "interrupted" {
                    self.details.rounds[index].status = "inProgress".into();
                }
            }
            return;
        }
        let message = if kind == "response_item" && event == "message" && p["role"] == "user" {
            Some(preview::message_text(p))
        } else if kind == "event_msg" && event == "user_message" {
            p["message"]
                .as_str()
                .or_else(|| p["text"].as_str())
                .map(str::to_owned)
        } else if kind == "event_msg"
            && event == "item_completed"
            && p["item"]["type"] == "UserMessage"
        {
            Some(preview::message_text(&p["item"]))
        } else {
            None
        };
        if let Some(message) = message.filter(|s| {
            !s.trim().is_empty()
                && preview::system_label(s).is_none()
                && !s
                    .trim_start()
                    .starts_with("<external_codex_apps_open_page>")
                && !s.trim_start().starts_with("<codex_internal_context")
        }) {
            let mut index = turn
                .and_then(|t| self.turns.get(t))
                .copied()
                .unwrap_or_else(|| {
                    let old = self.current;
                    let index = self.current(at);
                    if old != Some(index) {
                        self.active_turn = None;
                    }
                    index
                });
            let active = turn.or(self.active_turn.as_deref()).map(str::to_owned);
            let text = message.trim().to_ascii_lowercase();
            let continuing = text.starts_with("继续")
                || text.starts_with("接着")
                || text == "continue"
                || text.starts_with("continue ")
                || text == "resume"
                || text.starts_with("resume ");
            if let Some(previous) = active
                .as_ref()
                .and_then(|turn| self.resume.remove(turn))
                .filter(|_| continuing)
            {
                if previous < index
                    && self.details.rounds[index].prompt.is_empty()
                    && index + 1 == self.details.rounds.len()
                {
                    self.details.rounds.pop();
                    for value in self.turns.values_mut() {
                        if *value == index {
                            *value = previous;
                        }
                    }
                    for value in self.resume.values_mut() {
                        if *value == index {
                            *value = previous;
                        }
                    }
                    index = previous;
                    self.current = Some(index);
                    self.details.rounds[index].status = "inProgress".into();
                    self.details.rounds[index].completed_at = None;
                }
            }
            if let Some(turn) = turn {
                self.turns.insert(turn.into(), index);
            }
            let round = &mut self.details.rounds[index];
            if round.prompt.is_empty() {
                round.prompt = message
                    .split_whitespace()
                    .flat_map(|word| word.chars().chain(std::iter::once(' ')))
                    .take(160)
                    .collect::<String>()
                    .trim_end()
                    .to_owned();
                round.started_at = at.or(round.started_at);
            }
            return;
        }
        let index = match turn.or(self.active_turn.as_deref()) {
            Some(turn) => self.turns.get(turn).copied(),
            None => self.current,
        };
        if kind == "token_usage_record" {
            if let (Some(response), Some(index)) = (p["response_id"].as_str(), index) {
                self.details
                    .requests
                    .entry(response.into())
                    .or_insert(index);
            }
        } else if let Some(index) = index {
            let final_message = kind == "response_item"
                && event == "message"
                && p["role"] == "assistant"
                && p["phase"].as_str().or(p["channel"].as_str()) == Some("final")
                || kind == "event_msg"
                    && event == "item_completed"
                    && p["item"]["type"] == "AgentMessage"
                    && p["item"]["phase"] == "final"
                || kind == "event_msg" && event == "agent_message" && p["phase"] == "final";
            let round = &mut self.details.rounds[index];
            if (final_message
                || kind == "event_msg"
                    && event == "task_complete"
                    && round.status != "interrupted"
                    && turn
                        .or(self.active_turn.as_deref())
                        .is_none_or(|t| !self.aborted.contains(t)))
                && turn
                    .or(self.active_turn.as_deref())
                    .is_none_or(|t| !self.aborted.contains(t))
            {
                round.status = "completed".into();
                round.completed_at = round.completed_at.or(at);
            } else if kind == "event_msg" && event == "turn_aborted" && round.status != "completed"
            {
                round.status = "interrupted".into();
                if let Some(turn) = turn.or(self.active_turn.as_deref()) {
                    self.aborted.insert(turn.into());
                }
            }
        }
    }

    pub(crate) fn attach_requests(&mut self, data: &crate::metrics::ChatMetrics) {
        for turn in &data.turns {
            let Some(&index) = self.turns.get(&turn.turn_id) else {
                continue;
            };
            for request in &turn.requests {
                if matches!(
                    request.excluded_reason.as_deref(),
                    Some("用量冲突" | "用量字段无效")
                ) {
                    continue;
                }
                let (Some(input), Some(cached), Some(output)) = (
                    request.input_tokens,
                    request.cached_input_tokens,
                    request.output_tokens,
                ) else {
                    continue;
                };
                if cached > input
                    || request
                        .reasoning_tokens
                        .is_some_and(|reasoning| reasoning > output)
                {
                    continue;
                }
                self.details
                    .requests
                    .insert(request.response_id.clone(), index);
                self.details.rounds[index]
                    .request_times
                    .push(request.settled_at);
            }
        }
    }
}

pub(super) fn read(home: &Path, id: &str) -> Result<(Details, String), String> {
    let source = Connection::open_with_flags(
        home.join("state_5.sqlite"),
        OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .map_err(|e| e.to_string())?;
    let path: PathBuf = source
        .query_row("SELECT rollout_path FROM threads WHERE id=?1", [id], |r| {
            r.get::<_, String>(0)
        })
        .map(PathBuf::from)
        .map_err(|e| e.to_string())?;
    let identity = super::super::context::identity(home)?;
    let db = database(home)?;
    let cached: Option<(String, String)> = db
        .query_row(
            "SELECT signature,data FROM usage_round_details WHERE thread_id=?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    if let Some((stamp, data)) = cached {
        if let Some(signature) = stamp.strip_prefix("question-rounds-v2|") {
            if unchanged(signature, &path) {
                if let Ok(details) = serde_json::from_str(&data) {
                    return Ok((details, stamp));
                }
            }
        }
    }
    let before = signature(home, id, &path)?;
    let (_, details) = crate::metrics::analysis::read(home, id)?;
    if before != signature(home, id, &path)? {
        return Err("读取期间日志有更新，请重新展开".into());
    }
    if super::super::context::identity(home)? != identity {
        return Err("账号或日志来源已切换，请重新读取".into());
    }
    let stamp = format!(
        "question-rounds-v2|{}",
        serde_json::to_string(&before).map_err(|e| e.to_string())?
    );
    db.execute(
        "INSERT OR REPLACE INTO usage_round_details VALUES(?1,?2,?3)",
        params![
            id,
            stamp,
            serde_json::to_string(&details).map_err(|e| e.to_string())?
        ],
    )
    .map_err(|e| e.to_string())?;
    Ok((details, stamp))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rollout_log;
    use serde_json::json;
    use std::io::{BufRead, BufReader, Read};

    #[test]
    fn independent_turns_do_not_merge_when_a_previous_reply_is_missing() {
        for interrupted in [false, true] {
            let mut parser = Parser::default();
            for record in [
                json!({"type":"session_meta","payload":{"id":"main"}}),
                json!({"type":"event_msg","payload":{"type":"task_started","turn_id":"one"}}),
                json!({"type":"event_msg","payload":{"type":"user_message","message":"First question"}}),
                json!({"type":"event_msg","payload":{"type":if interrupted {"turn_aborted"} else {"unrelated"},"turn_id":"one"}}),
                json!({"type":"event_msg","payload":{"type":"task_started","turn_id":"two"}}),
                json!({"type":"event_msg","payload":{"type":"user_message","message":"Independent question"}}),
                json!({"type":"token_usage_record","payload":{"turn_id":"unseen","response_id":"orphan"}}),
            ] {
                parser.apply("main", &record);
            }
            assert_eq!(parser.details.rounds.len(), 2);
            assert_eq!(parser.details.rounds[1].prompt, "Independent question");
            assert!(!parser.details.requests.contains_key("orphan"));
        }
    }

    #[test]
    fn goal_continuation_reuses_the_user_question_and_duplicate_end_does_not_extend_it() {
        let mut parser = Parser::default();
        for record in [
            json!({"type":"session_meta","payload":{"id":"main"}}),
            json!({"type":"event_msg","payload":{"type":"task_started","turn_id":"one"}}),
            json!({"type":"event_msg","payload":{"type":"user_message","message":"First question"}}),
            json!({"timestamp":"1970-01-01T00:00:01Z","type":"event_msg","payload":{"type":"task_complete","turn_id":"one"}}),
            json!({"type":"event_msg","payload":{"type":"task_started","turn_id":"goal","turn_attribution":{"turn_trigger":"goal"}}}),
            json!({"type":"response_item","payload":{"type":"message","role":"user","content":[{"text":"<codex_internal_context source=\"goal\">continue"}]}}),
            json!({"type":"token_usage_record","payload":{"turn_id":"goal","response_id":"continued"}}),
            json!({"timestamp":"1970-01-01T00:00:02Z","type":"response_item","payload":{"type":"message","role":"assistant","phase":"final"}}),
            json!({"timestamp":"1970-01-01T00:00:03Z","type":"event_msg","payload":{"type":"task_complete","turn_id":"goal"}}),
        ] {
            parser.apply("main", &record);
        }
        assert_eq!(parser.details.rounds.len(), 1);
        assert_eq!(parser.details.rounds[0].prompt, "First question");
        assert_eq!(parser.details.rounds[0].completed_at, Some(2000));
        assert_eq!(parser.details.requests["continued"], 0);
    }

    #[test]
    #[ignore = "reads selected real logs and compares request ownership with conversation details"]
    fn live_round_request_reconciliation() {
        let home = crate::source::codex_home().unwrap();
        let ids = std::env::var("CODEX_PANE_ROUND_VERIFY_IDS").unwrap_or_else(|_| {
            "01a0ff60-165e-7ff1-89c5-4d58b94ce669,01a162f3-1e03-7ff2-b959-44026ac0ff7b".into()
        });
        let catalog = Connection::open_with_flags(
            home.join("state_5.sqlite"),
            OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .unwrap();
        let mut missing_total = 0;
        for id in ids.split(',') {
            let path: String = catalog
                .query_row("SELECT rollout_path FROM threads WHERE id=?1", [id], |r| {
                    r.get(0)
                })
                .unwrap();
            let before = signature(&home, id, Path::new(&path)).unwrap();
            let data = crate::metrics::read_own(&home, id).unwrap();
            let segments = preview::metrics_segments(&home, id, Path::new(&path)).unwrap();
            let mut parser = Parser::default();
            let mut compressed = 0;
            let mut interrupted = 0;
            let mut legacy = 0;
            for (path, end) in &segments {
                let log = rollout_log::open(path).unwrap();
                compressed += usize::from(log.compressed);
                for line in BufReader::new(log.reader_at(0).unwrap().take(*end)).lines() {
                    let record: Value = serde_json::from_str(&line.unwrap()).unwrap();
                    interrupted += usize::from(record["payload"]["type"] == "turn_aborted");
                    legacy += usize::from(record["payload"]["type"] == "token_count");
                    parser.apply(id, &record);
                }
            }
            parser.attach_requests(&data);
            assert_eq!(
                before,
                signature(&home, id, Path::new(&path)).unwrap(),
                "selected log changed during verification"
            );
            let requests: Vec<_> = data
                .turns
                .iter()
                .flat_map(|t| &t.requests)
                .filter(|r| {
                    r.input_tokens.is_some()
                        && !matches!(
                            r.excluded_reason.as_deref(),
                            Some("用量冲突" | "用量字段无效")
                        )
                })
                .collect();
            let missing = requests
                .iter()
                .filter(|r| !parser.details.requests.contains_key(&r.response_id))
                .count();
            println!("id={id} segments={} compressed={compressed} interrupted={interrupted} token_count={legacy} detail_turns={} question_rounds={} own_requests={} missing_round_mapping={missing}",segments.len(),data.turns.len(),parser.details.rounds.len(),requests.len());
            missing_total += missing;
            // Exercise real report filtering in an isolated App cache; source logs/catalog stay read-only.
            let isolated = tempfile::tempdir().unwrap();
            super::super::super::context::confirm(isolated.path(), Some("verification-only"))
                .unwrap();
            let catalog = Connection::open(isolated.path().join("state_5.sqlite")).unwrap();
            catalog.execute_batch("CREATE TABLE threads(id TEXT,title TEXT,rollout_path TEXT,created_at INTEGER,updated_at INTEGER,thread_source TEXT);").unwrap();
            catalog
                .execute(
                    "INSERT INTO threads VALUES(?1,?2,?3,?4,?5,'user')",
                    params![
                        id,
                        data.title,
                        path,
                        data.created_at.map(|at| at / 1000),
                        data.updated_at.map(|at| at / 1000)
                    ],
                )
                .unwrap();
            let mut cached = data.clone();
            cached.usage_batches = data
                .turns
                .iter()
                .map(|turn| {
                    (
                        turn.model.clone(),
                        turn.service_tier.clone(),
                        turn.requests.clone(),
                    )
                })
                .collect();
            super::super::reuse(isolated.path(), &cached).unwrap();
            let db = database(isolated.path()).unwrap();
            let stamp = format!(
                "question-rounds-v2|{}",
                serde_json::to_string(&before).unwrap()
            );
            db.execute(
                "INSERT OR REPLACE INTO usage_round_details VALUES(?1,?2,?3)",
                params![id, stamp, serde_json::to_string(&parser.details).unwrap()],
            )
            .unwrap();
            let latest = requests.iter().filter_map(|r| r.settled_at).max().unwrap();
            let from = latest.saturating_sub(2 * 86_400_000);
            let to = latest + 1;
            let expected: Vec<_> = requests
                .iter()
                .filter(|r| r.settled_at.is_some_and(|at| at >= from && at < to))
                .collect();
            let report = super::super::report(
                isolated.path(),
                super::super::Query {
                    from: Some(from),
                    to: Some(to),
                    group: Some("round".into()),
                    source_thread_id: Some(id.into()),
                    ..Default::default()
                },
            )
            .unwrap();
            let input = expected
                .iter()
                .map(|r| r.input_tokens.unwrap())
                .sum::<u64>();
            let output = expected
                .iter()
                .map(|r| r.output_tokens.unwrap())
                .sum::<u64>();
            assert_eq!(report.totals.input, input);
            assert_eq!(report.totals.output, output);
            assert_eq!(
                report.rows.iter().map(|r| r.totals.requests).sum::<usize>(),
                expected.len()
            );
            assert_eq!(
                report
                    .model_totals
                    .iter()
                    .map(|r| r.totals.input + r.totals.output)
                    .sum::<u64>(),
                input + output
            );
            let unknown = report
                .rows
                .iter()
                .filter(|r| r.round_number.is_none())
                .map(|r| r.totals.requests)
                .sum::<usize>();
            println!("id={id} selected_requests={} rounds_plus_unknown={} unknown_requests={unknown} input={input} output={output} model_tokens={}",expected.len(),report.rows.iter().map(|r|r.totals.requests).sum::<usize>(),input+output);
        }
        assert_eq!(
            missing_total, 0,
            "settled requests visible in conversation details must retain their round ownership"
        );
    }

    #[test]
    fn interruption_and_steering_stay_with_the_first_question_until_final_reply() {
        let mut parser = Parser::default();
        for record in [
            json!({"type":"session_meta","payload":{"id":"parent"}}),
            json!({"type":"event_msg","payload":{"type":"user_message","message":"inherited"}}),
            json!({"type":"token_usage_record","payload":{"response_id":"foreign"}}),
            json!({"type":"session_meta","payload":{"id":"main"}}),
            json!({"type":"event_msg","payload":{"type":"task_started","turn_id":"one"}}),
            json!({"type":"event_msg","payload":{"type":"user_message","message":"# AGENTS.md instructions\ncontext"}}),
            json!({"type":"event_msg","payload":{"type":"user_message","message":"first question"}}),
            json!({"type":"response_item","payload":{"type":"message","role":"user","content":[{"text":"first question"}]}}),
            json!({"type":"token_usage_record","payload":{"turn_id":"one","response_id":"a"}}),
            json!({"type":"event_msg","payload":{"type":"turn_aborted","turn_id":"one"}}),
            json!({"type":"event_msg","payload":{"type":"task_complete","turn_id":"one"}}),
            json!({"type":"event_msg","payload":{"type":"task_started","turn_id":"resume"}}),
            json!({"type":"event_msg","payload":{"type":"user_message","message":"continue and fix this"}}),
            json!({"type":"event_msg","payload":{"type":"task_complete","turn_id":"one"}}),
            json!({"type":"response_item","payload":{"type":"message","role":"assistant","channel":"commentary","content":[{"text":"progress"}]}}),
            json!({"type":"event_msg","payload":{"type":"user_message","message":"another steer"}}),
            json!({"type":"token_usage_record","payload":{"turn_id":"resume","response_id":"b"}}),
            json!({"type":"event_msg","payload":{"type":"item_completed","item":{"type":"AgentMessage","phase":"final"}}}),
            json!({"type":"event_msg","payload":{"type":"task_complete","turn_id":"resume"}}),
            json!({"type":"event_msg","payload":{"type":"user_message","message":"second question"}}),
            json!({"type":"event_msg","payload":{"type":"task_started","turn_id":"two"}}),
            json!({"type":"token_usage_record","payload":{"turn_id":"one","response_id":"late"}}),
            json!({"type":"token_usage_record","payload":{"turn_id":"two","response_id":"c"}}),
            json!({"type":"token_usage_record","payload":{"thread_id":"child","turn_id":"two","response_id":"child"}}),
        ] {
            parser.apply("main", &record);
        }
        assert_eq!(parser.details.rounds.len(), 2);
        assert_eq!(parser.details.rounds[0].prompt, "first question");
        assert_eq!(parser.details.rounds[0].status, "completed");
        assert_eq!(parser.details.rounds[1].prompt, "second question");
        assert_eq!(parser.details.requests["a"], 0);
        assert_eq!(parser.details.requests["b"], 0);
        assert_eq!(parser.details.requests["late"], 0);
        assert_eq!(parser.details.requests["c"], 1);
        assert_eq!(parser.details.requests.len(), 4);
    }
}
