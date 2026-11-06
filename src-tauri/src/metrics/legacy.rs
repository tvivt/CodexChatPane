use super::{text, timestamp, Parser, Usage};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashSet;

type Identity = (u64, u64, Option<String>, [u64; 3], [u64; 3]);

#[derive(Clone, Serialize, Deserialize)]
struct Snapshot {
    at: i64,
    turn: String,
    model: Option<String>,
    tier: Option<String>,
    usage: Value,
    response_id: String,
    identity: Option<Identity>,
    turn_identity: Option<Identity>,
}

#[derive(Clone, Serialize, Deserialize)]
pub(super) struct Collector {
    id: String,
    active: String,
    turn: Option<String>,
    model: Option<String>,
    tier: Option<String>,
    previous: Option<Usage>,
    turn_base: Option<Usage>,
    snapshots: Vec<Snapshot>,
    gaps: bool,
    epoch: u64,
    legacy_epoch: u64,
    modern_epoch: u64,
    modern_previous: Option<[u64; 3]>,
    modern_responses: HashSet<String>,
    modern: HashSet<Identity>,
    compactions: HashSet<String>,
    thread_compaction: [u64; 3],
    turn_compaction: [u64; 3],
}

impl Collector {
    pub fn new(id: &str) -> Self {
        Self {
            id: id.into(),
            active: String::new(),
            turn: None,
            model: None,
            tier: None,
            previous: None,
            turn_base: None,
            snapshots: Vec::new(),
            gaps: false,
            epoch: 0,
            legacy_epoch: 0,
            modern_epoch: 0,
            modern_previous: None,
            modern_responses: HashSet::new(),
            modern: HashSet::new(),
            compactions: HashSet::new(),
            thread_compaction: [0; 3],
            turn_compaction: [0; 3],
        }
    }
    pub fn apply(&mut self, record: &Value) {
        let p = &record["payload"];
        if record["type"] == "session_meta" {
            let next = text(&p["id"]);
            if next != self.active {
                self.epoch += 1;
                self.legacy_epoch = 0;
                self.modern_epoch = 0;
                self.modern_previous = None;
                self.modern_responses.clear();
                self.compactions.clear();
                self.thread_compaction = [0; 3];
                self.turn_compaction = [0; 3];
                self.previous = None;
                self.turn_base = None;
                self.turn = None;
                self.model = None;
                self.tier = None;
            }
            self.active = next.into();
            return;
        }
        let explicit = p["thread_id"].as_str().or_else(|| {
            (record["type"] == "compacted")
                .then(|| p["latest_token_usage_record"]["thread_id"].as_str())
                .flatten()
        });
        if explicit.is_some_and(|id| id != self.id) {
            return;
        }
        if explicit == Some(self.id.as_str()) && self.active != self.id {
            self.epoch += 1;
            self.legacy_epoch = 0;
            self.modern_epoch = 0;
            self.modern_previous = None;
            self.modern_responses.clear();
            self.compactions.clear();
            self.thread_compaction = [0; 3];
            self.turn_compaction = [0; 3];
            self.active = self.id.clone();
            self.previous = None;
            self.turn_base = None;
            self.turn = None;
            self.model = None;
            self.tier = None;
        }
        if self.active != self.id {
            return;
        }
        if record["type"] == "turn_context" {
            self.model = p["model"].as_str().map(str::to_owned);
        }
        if p["type"] == "thread_settings_applied" {
            self.tier = p["thread_settings"]["service_tier"]
                .as_str()
                .map(str::to_owned);
        }
        if let Some(turn) = p["turn_id"]
            .as_str()
            .or_else(|| p["internal_chat_message_metadata_passthrough"]["turn_id"].as_str())
        {
            if self.turn.as_deref() != Some(turn) {
                self.turn_base = self.previous.clone();
                self.turn_compaction = [0; 3];
            }
            self.turn = Some(turn.into());
        }
        if record["type"] == "token_usage_record" {
            if text(&p["response_id"]).is_empty() || self.turn.is_none() {
                return;
            }
            if !self.modern_responses.insert(text(&p["response_id"]).into()) {
                return;
            }
            if let Ok(usage) = serde_json::from_value::<Usage>(p["usage"].clone()) {
                if let Some(total) =
                    serde_json::from_value::<Usage>(p["thread_token_usage"].clone())
                        .ok()
                        .and_then(|total| counts(&total))
                {
                    if self.modern_previous.is_some_and(|old| falls(&total, &old)) {
                        self.modern_epoch += 1;
                        self.thread_compaction = [0; 3];
                        self.turn_compaction = [0; 3];
                        self.compactions.clear();
                    }
                    self.modern_previous = Some(total);
                }
                for (field, turn, offset) in [
                    ("thread_token_usage", None, self.thread_compaction),
                    ("turn_token_usage", self.turn.clone(), self.turn_compaction),
                ] {
                    let Ok(total) = serde_json::from_value::<Usage>(p[field].clone()) else {
                        continue;
                    };
                    for total in [Some(total.clone()), subtract(&total, offset)]
                        .into_iter()
                        .flatten()
                    {
                        if let Some(identity) =
                            self.identity(&usage, &total, turn.as_deref(), self.modern_epoch)
                        {
                            self.modern.insert(identity);
                        }
                    }
                }
            }
            return;
        }
        if record["type"] == "compacted" {
            let latest = &p["latest_token_usage_record"];
            let response = text(&p["compaction_response_id"]);
            if !response.is_empty()
                && latest["thread_id"] == self.id
                && latest["response_id"] == response
                && !self.compactions.contains(response)
            {
                if let Ok(usage) = serde_json::from_value::<Usage>(latest["usage"].clone()) {
                    if let Some(counts) = counts(&usage) {
                        let add = |base: [u64; 3]| -> Option<[u64; 3]> {
                            Some([
                                base[0].checked_add(counts[0])?,
                                base[1].checked_add(counts[1])?,
                                base[2].checked_add(counts[2])?,
                            ])
                        };
                        if let Some(thread) = add(self.thread_compaction) {
                            let turn = if latest["turn_id"].as_str() == self.turn.as_deref() {
                                add(self.turn_compaction)
                            } else {
                                Some(self.turn_compaction)
                            };
                            if let Some(turn) = turn {
                                self.thread_compaction = thread;
                                self.turn_compaction = turn;
                                self.compactions.insert(response.into());
                            }
                        }
                    }
                }
            }
            return;
        }
        if record["type"] != "event_msg" || p["type"] != "token_count" {
            return;
        }
        let (Ok(total), Ok(last)) = (
            serde_json::from_value::<Usage>(p["info"]["total_token_usage"].clone()),
            serde_json::from_value::<Usage>(p["info"]["last_token_usage"].clone()),
        ) else {
            return;
        };
        if self.previous.as_ref() == Some(&total) {
            return;
        }
        if self
            .previous
            .as_ref()
            .and_then(counts)
            .zip(counts(&total))
            .is_some_and(|(old, current)| falls(&current, &old))
        {
            self.legacy_epoch += 1;
            self.turn_base = None;
        }
        let old = self.previous.replace(total.clone()).unwrap_or(Usage {
            input_tokens: Some(0),
            cached_input_tokens: Some(0),
            output_tokens: Some(0),
            reasoning_output_tokens: Some(0),
            total_tokens: Some(0),
        });
        let valid = [
            (old.input_tokens, last.input_tokens, total.input_tokens),
            (
                old.cached_input_tokens,
                last.cached_input_tokens,
                total.cached_input_tokens,
            ),
            (old.output_tokens, last.output_tokens, total.output_tokens),
        ]
        .into_iter()
        .all(|(old, last, total)| {
            old.zip(last)
                .and_then(|(a, b)| a.checked_add(b))
                .is_some_and(|sum| Some(sum) == total)
        }) && last.cached_input_tokens <= last.input_tokens
            && last.reasoning_output_tokens <= last.output_tokens;
        let Some(at) = timestamp(&record["timestamp"]) else {
            self.gaps = true;
            return;
        };
        if !valid {
            self.gaps = true;
            return;
        }
        let identity = self.identity(&last, &total, None, self.legacy_epoch);
        let Some((epoch, counter_epoch, _, _, endpoint)) = identity.as_ref() else {
            self.gaps = true;
            return;
        };
        let response_id = format!(
            "legacy-{epoch}-{counter_epoch}-{}-{}-{}-{}",
            self.turn.as_deref().unwrap_or("unknown"),
            endpoint[0],
            endpoint[1],
            endpoint[2]
        );
        self.snapshots.push(Snapshot {
            at,
            turn: self.turn.clone().unwrap_or_else(|| format!("legacy-{at}")),
            model: self.model.clone(),
            tier: self.tier.clone(),
            usage: p["info"]["last_token_usage"].clone(),
            identity,
            turn_identity: self.turn_base.as_ref().and_then(|base| {
                let relative = Usage {
                    input_tokens: total.input_tokens?.checked_sub(base.input_tokens?),
                    cached_input_tokens: total
                        .cached_input_tokens?
                        .checked_sub(base.cached_input_tokens?),
                    output_tokens: total.output_tokens?.checked_sub(base.output_tokens?),
                    ..Default::default()
                };
                self.identity(&last, &relative, self.turn.as_deref(), self.legacy_epoch)
            }),
            response_id,
        });
    }
    fn identity(
        &self,
        usage: &Usage,
        total: &Usage,
        turn: Option<&str>,
        counter_epoch: u64,
    ) -> Option<Identity> {
        Some((
            self.epoch,
            counter_epoch,
            turn.map(str::to_owned),
            counts(usage)?,
            counts(total)?,
        ))
    }
    pub fn attach(self, parser: &mut Parser) {
        let has_modern = !parser.usages.is_empty();
        let mut added = false;
        for row in self.snapshots {
            // Cumulative endpoints identify the settlement even when the twin arrives much later.
            if row
                .identity
                .as_ref()
                .into_iter()
                .chain(row.turn_identity.as_ref())
                .any(|identity| self.modern.contains(identity))
            {
                continue;
            }
            let state = parser.turn(&row.turn);
            if state.turn.model.is_none() {
                state.turn.model = row.model;
            }
            if state.turn.service_tier.is_none() {
                state.turn.service_tier = row.tier;
            }
            let mut usage = row.usage;
            usage["cache_write_tokens"] = usage["cache_write_input_tokens"].clone();
            parser.usage(
                &row.turn,
                &json!({"response_id":row.response_id,"usage":usage}),
                Some(row.at),
            );
            added = true;
        }
        if added {
            parser.warning("旧日志用量来自逐次快照校验，缺少请求计时");
        }
        if self.gaps && !has_modern {
            parser.warning("部分旧快照缺少连续基线，未分摊累计用量");
        }
    }
}

fn counts(usage: &Usage) -> Option<[u64; 3]> {
    Some([
        usage.input_tokens?,
        usage.cached_input_tokens?,
        usage.output_tokens?,
    ])
}

fn falls(current: &[u64; 3], previous: &[u64; 3]) -> bool {
    current
        .iter()
        .zip(previous)
        .any(|(current, previous)| current < previous)
}

fn subtract(total: &Usage, offset: [u64; 3]) -> Option<Usage> {
    Some(Usage {
        input_tokens: Some(total.input_tokens?.checked_sub(offset[0])?),
        cached_input_tokens: Some(total.cached_input_tokens?.checked_sub(offset[1])?),
        output_tokens: Some(total.output_tokens?.checked_sub(offset[2])?),
        ..Default::default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_rollback_separates_reused_endpoints_across_checkpoints() {
        for known_turn in [false, true] {
            for spaced in [false, true] {
                let mut old = Collector::new("main");
                let mut parser = Parser::new("main");
                old.apply(&json!({"type":"session_meta","payload":{"id":"main"}}));
                if known_turn {
                    old.apply(&json!({"type":"turn_context","payload":{"turn_id":"own"}}));
                }
                for (i, (end, last)) in [(10, 10), (20, 10), (3, 3), (8, 5), (20, 12), (20, 12)]
                    .into_iter()
                    .enumerate()
                {
                    let at = format!("2026-06-15T01:00:00.{:03}Z", if spaced { i } else { 0 });
                    old.apply(&snapshot(end * 10, end, last * 10, last, &at));
                    if i == 2 {
                        old = serde_json::from_str(&serde_json::to_string(&old).unwrap()).unwrap();
                    }
                }
                old.attach(&mut parser);
                let requests: Vec<_> = parser
                    .turns
                    .values()
                    .flat_map(|t| &t.turn.requests)
                    .collect();
                assert_eq!(requests.len(), 4);
                assert_eq!(
                    requests
                        .iter()
                        .map(|r| &r.response_id)
                        .collect::<HashSet<_>>()
                        .len(),
                    4
                );
                assert_eq!(
                    requests
                        .iter()
                        .map(|r| r.output_tokens.unwrap())
                        .sum::<u64>(),
                    37
                );
                assert!(requests
                    .iter()
                    .all(|r| r.excluded_reason.as_deref() != Some("用量冲突")));
            }
        }
    }

    #[test]
    fn modern_and_legacy_rollbacks_match_only_within_their_counter_epoch() {
        let native = |id: &str, end: u64, last: u64| json!({"type":"token_usage_record","timestamp":"2026-06-15T01:00:00Z","payload":{"thread_id":"main","turn_id":"own","response_id":id,"usage":{"input_tokens":last*10,"cached_input_tokens":0,"output_tokens":last},"thread_token_usage":{"input_tokens":end*10,"cached_input_tokens":0,"output_tokens":end}}});
        let feed = |old: &mut Collector, parser: &mut Parser, record: Value| {
            old.apply(&record);
            parser.apply(record);
        };
        for modern_first in [false, true] {
            let mut old = Collector::new("main");
            let mut parser = Parser::new("main");
            old.apply(&json!({"type":"session_meta","payload":{"id":"main"}}));
            for (id, end, last) in [("a", 10, 10), ("b", 20, 10)] {
                feed(&mut old, &mut parser, native(id, end, last));
                old.apply(&snapshot(
                    end * 10,
                    end,
                    last * 10,
                    last,
                    "2026-06-15T01:00:00Z",
                ));
            }
            if modern_first {
                feed(&mut old, &mut parser, native("c", 8, 5));
            }
            old.apply(&snapshot(30, 3, 30, 3, "2026-06-15T01:00:00Z"));
            old = serde_json::from_str(&serde_json::to_string(&old).unwrap()).unwrap();
            old.apply(&snapshot(80, 8, 50, 5, "2026-06-15T01:00:00Z"));
            if !modern_first {
                feed(&mut old, &mut parser, native("c", 8, 5));
            }
            feed(&mut old, &mut parser, native("d", 20, 12));
            old.apply(&snapshot(200, 20, 120, 12, "2026-06-15T01:00:00Z"));
            feed(&mut old, &mut parser, native("a", 10, 10));
            feed(&mut old, &mut parser, native("e", 25, 5));
            old.apply(&snapshot(250, 25, 50, 5, "2026-06-15T01:00:00Z"));
            old.attach(&mut parser);
            let requests = &parser.turns["own"].turn.requests;
            assert_eq!(
                requests
                    .iter()
                    .map(|r| r.response_id.as_str())
                    .collect::<HashSet<_>>(),
                HashSet::from(["a", "b", "c", "d", "e"])
            );
            assert_eq!(
                requests
                    .iter()
                    .map(|r| r.output_tokens.unwrap())
                    .sum::<u64>(),
                42
            );
        }
        let mut old = Collector::new("main");
        let mut parser = Parser::new("main");
        old.apply(&json!({"type":"session_meta","payload":{"id":"main"}}));
        old.apply(&json!({"type":"turn_context","payload":{"turn_id":"own"}}));
        feed(&mut old, &mut parser, native("before", 20, 10));
        for (end, last) in [(10, 10), (20, 10), (3, 3), (10, 7), (20, 10)] {
            old.apply(&snapshot(
                end * 10,
                end,
                last * 10,
                last,
                "2026-06-15T01:00:00Z",
            ));
        }
        old.attach(&mut parser);
        let requests = &parser.turns["own"].turn.requests;
        assert_eq!(
            requests.len(),
            4,
            "old modern endpoints cannot erase legacy-only requests after rollback"
        );
        assert_eq!(
            requests
                .iter()
                .map(|r| r.output_tokens.unwrap())
                .sum::<u64>(),
            37
        );
    }

    fn snapshot(input: u64, output: u64, last_input: u64, last_output: u64, at: &str) -> Value {
        json!({"type":"event_msg","timestamp":at,"payload":{"type":"token_count","info":{
            "total_token_usage":{"input_tokens":input,"cached_input_tokens":0,"output_tokens":output},
            "last_token_usage":{"input_tokens":last_input,"cached_input_tokens":0,"output_tokens":last_output}}}})
    }
    #[test]
    fn validates_deltas_skips_inheritance_duplicates_gaps_and_modern_twins() {
        let mut old = Collector::new("main");
        old.apply(&json!({"type":"session_meta","payload":{"id":"parent"}}));
        old.apply(&snapshot(9000, 900, 9000, 900, "2026-06-15T01:00:00Z"));
        old.apply(&json!({"type":"session_meta","payload":{"id":"main"}}));
        old.apply(&snapshot(100, 10, 100, 10, "2026-06-15T01:01:00Z"));
        old.apply(&snapshot(100, 10, 100, 10, "2026-06-15T01:01:01Z"));
        old.apply(&snapshot(500, 50, 200, 20, "2026-06-15T01:02:00Z"));
        old.apply(&snapshot(700, 70, 200, 20, "2026-06-15T01:03:00Z"));
        let mut parser = Parser::new("main");
        let modern = json!({"type":"token_usage_record","timestamp":"2026-06-15T01:03:00.001Z","payload":{"thread_id":"main","turn_id":"modern","response_id":"modern","usage":{"input_tokens":200,"cached_input_tokens":0,"output_tokens":20},"thread_token_usage":{"input_tokens":700,"cached_input_tokens":0,"output_tokens":70}}});
        old.apply(&modern);
        parser.apply(modern);
        old.attach(&mut parser);
        let requests: Vec<_> = parser
            .turns
            .values()
            .flat_map(|t| &t.turn.requests)
            .collect();
        assert_eq!(requests.len(), 2);
        assert_eq!(
            requests
                .iter()
                .map(|r| r.input_tokens.unwrap())
                .sum::<u64>(),
            300
        );
        assert!(
            requests.iter().all(|r| !r.included),
            "legacy usage cannot invent output timing"
        );
    }

    #[test]
    fn explicit_compaction_usage_aligns_counters_without_counting_it_twice() {
        let mut old = Collector::new("main");
        let mut parser = Parser::new("main");
        old.apply(&json!({"type":"session_meta","payload":{"id":"main"}}));
        old.apply(&json!({"type":"turn_context","payload":{"turn_id":"own"}}));
        let request = |id: &str, input: u64, total: u64| json!({"type":"token_usage_record","timestamp":"2026-06-15T01:00:00Z","payload":{"thread_id":"main","turn_id":"own","response_id":id,"usage":{"input_tokens":input,"cached_input_tokens":0,"output_tokens":input/10},"thread_token_usage":{"input_tokens":total,"cached_input_tokens":0,"output_tokens":total/10},"turn_token_usage":{"input_tokens":total,"cached_input_tokens":0,"output_tokens":total/10}}});
        let first = request("first", 100, 100);
        old.apply(&first);
        parser.apply(first);
        old.apply(&snapshot(100, 10, 100, 10, "2026-06-15T01:10:00Z"));
        let compact = request("compact", 900, 1000);
        old.apply(&compact);
        parser.apply(compact.clone());
        let event = json!({"type":"compacted","payload":{"compaction_response_id":"compact","latest_token_usage_record":compact["payload"]}});
        old.apply(&event);
        old.apply(&event);
        old = serde_json::from_str(&serde_json::to_string(&old).unwrap()).unwrap();
        let next = request("next", 50, 1050);
        old.apply(&next);
        parser.apply(next);
        old.apply(&snapshot(150, 15, 50, 5, "2026-06-15T01:20:00Z"));
        old.apply(&snapshot(200, 20, 50, 5, "2026-06-15T01:21:00Z"));
        old.attach(&mut parser);
        let requests = &parser.turns["own"].turn.requests;
        assert_eq!(requests.len(), 4);
        assert_eq!(
            requests
                .iter()
                .map(|r| r.output_tokens.unwrap())
                .sum::<u64>(),
            110
        );
    }

    #[test]
    fn turn_cumulative_positions_match_after_history_baselines_diverge() {
        let mut old = Collector::new("main");
        let mut parser = Parser::new("main");
        old.apply(&json!({"type":"session_meta","payload":{"id":"main"}}));
        // A partial prior turn establishes the legacy baseline without inventing its missing usage.
        old.apply(&snapshot(900, 90, 1, 1, "2026-06-15T01:00:00Z"));
        old.apply(&json!({"type":"turn_context","payload":{"turn_id":"own"}}));
        let modern = json!({"type":"token_usage_record","timestamp":"2026-06-15T01:01:00Z","payload":{"thread_id":"main","turn_id":"own","response_id":"modern","usage":{"input_tokens":100,"cached_input_tokens":0,"output_tokens":10},"thread_token_usage":{"input_tokens":5000,"cached_input_tokens":0,"output_tokens":500},"turn_token_usage":{"input_tokens":100,"cached_input_tokens":0,"output_tokens":10}}});
        old.apply(&modern);
        parser.apply(modern);
        old = serde_json::from_str(&serde_json::to_string(&old).unwrap()).unwrap();
        old.apply(&snapshot(1000, 100, 100, 10, "2026-06-15T01:15:00Z"));
        old.apply(&snapshot(1100, 110, 100, 10, "2026-06-15T01:16:00Z"));
        old.attach(&mut parser);
        let requests = &parser.turns["own"].turn.requests;
        assert_eq!(requests.len(), 2);
        assert_eq!(
            requests
                .iter()
                .map(|r| r.output_tokens.unwrap())
                .sum::<u64>(),
            20
        );
    }

    #[test]
    fn cumulative_identity_matches_delayed_twins_but_keeps_equal_independent_requests() {
        for modern_first in [true, false] {
            let mut old = Collector::new("main");
            let mut parser = Parser::new("main");
            old.apply(&json!({"type":"session_meta","payload":{"id":"parent"}}));
            old.apply(&json!({"type":"token_usage_record","payload":{"thread_id":"parent","turn_id":"parent","response_id":"inherited","usage":{"input_tokens":100,"cached_input_tokens":0,"output_tokens":10},"thread_token_usage":{"input_tokens":200,"cached_input_tokens":0,"output_tokens":20}}}));
            old.apply(&json!({"type":"session_meta","payload":{"id":"main"}}));
            old.apply(&json!({"type":"turn_context","payload":{"turn_id":"own"}}));
            old.apply(&json!({"type":"token_usage_record","payload":{"turn_id":"own","usage":{"input_tokens":100,"cached_input_tokens":0,"output_tokens":10},"thread_token_usage":{"input_tokens":200,"cached_input_tokens":0,"output_tokens":20}}}));
            let modern = json!({"type":"token_usage_record","timestamp":"2026-06-15T01:00:00Z","payload":{"thread_id":"main","turn_id":"own","response_id":"modern","usage":{"input_tokens":100,"cached_input_tokens":0,"output_tokens":10},"thread_token_usage":{"input_tokens":100,"cached_input_tokens":0,"output_tokens":10}}});
            if modern_first {
                old.apply(&modern);
                parser.apply(modern.clone());
                old.apply(&json!({"type":"turn_context","payload":{"turn_id":"later"}}));
            }
            // Checkpoint between formats: late arrival also occurs across incremental reads.
            old = serde_json::from_str(&serde_json::to_string(&old).unwrap()).unwrap();
            old.apply(&snapshot(100, 10, 100, 10, "2026-06-15T01:15:00Z"));
            old.apply(&snapshot(200, 20, 100, 10, "2026-06-15T01:15:01Z"));
            if !modern_first {
                old = serde_json::from_str(&serde_json::to_string(&old).unwrap()).unwrap();
                old.apply(&modern);
                parser.apply(modern);
            }
            old.attach(&mut parser);
            let requests: Vec<_> = parser
                .turns
                .values()
                .flat_map(|t| &t.turn.requests)
                .collect();
            assert_eq!(requests.len(), 2);
            assert_eq!(
                requests
                    .iter()
                    .map(|r| r.output_tokens.unwrap())
                    .sum::<u64>(),
                20
            );
            assert_eq!(
                requests
                    .iter()
                    .filter(|r| r.response_id.starts_with("legacy-"))
                    .count(),
                1
            );
        }
    }
}
