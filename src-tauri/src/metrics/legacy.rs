use super::{text, timestamp, Parser, Usage};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Clone, Serialize, Deserialize)]
struct Snapshot {
    at: i64,
    turn: String,
    model: Option<String>,
    tier: Option<String>,
    usage: Value,
    counts: Usage,
}

#[derive(Clone, Serialize, Deserialize)]
pub(super) struct Collector {
    id: String,
    active: String,
    turn: Option<String>,
    model: Option<String>,
    tier: Option<String>,
    previous: Option<Usage>,
    snapshots: Vec<Snapshot>,
    gaps: bool,
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
            snapshots: Vec::new(),
            gaps: false,
        }
    }
    pub fn apply(&mut self, record: &Value) {
        let p = &record["payload"];
        if record["type"] == "session_meta" {
            let next = text(&p["id"]);
            if next != self.active {
                self.previous = None;
                self.turn = None;
                self.model = None;
                self.tier = None;
            }
            self.active = next.into();
            return;
        }
        let explicit = p["thread_id"].as_str();
        if explicit.is_some_and(|id| id != self.id) {
            return;
        }
        if explicit == Some(self.id.as_str()) && self.active != self.id {
            self.active = self.id.clone();
            self.previous = None;
            self.turn = None;
            self.model = None;
            self.tier = None;
        }
        if self.active != self.id {
            return;
        }
        if record["type"] == "turn_context" {
            self.turn = p["turn_id"].as_str().map(str::to_owned);
            self.model = p["model"].as_str().map(str::to_owned);
        }
        if p["type"] == "thread_settings_applied" {
            self.tier = p["thread_settings"]["service_tier"]
                .as_str()
                .map(str::to_owned);
        }
        if let Some(turn) = p["turn_id"].as_str() {
            self.turn = Some(turn.into());
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
        self.snapshots.push(Snapshot {
            at,
            turn: self.turn.clone().unwrap_or_else(|| format!("legacy-{at}")),
            model: self.model.clone(),
            tier: self.tier.clone(),
            usage: p["info"]["last_token_usage"].clone(),
            counts: last,
        });
    }
    pub fn attach(self, parser: &mut Parser) {
        let mut modern = std::collections::BTreeMap::<_, Vec<_>>::new();
        for request in parser.turns.values().flat_map(|t| &t.turn.requests) {
            modern
                .entry(request.settled_at.unwrap_or(i64::MIN))
                .or_default()
                .push((
                    request.input_tokens,
                    request.cached_input_tokens,
                    request.output_tokens,
                ));
        }
        let mut added = false;
        for row in self.snapshots {
            // New clients emit both forms for the same settled request, a few ms apart.
            let counts = (
                row.counts.input_tokens,
                row.counts.cached_input_tokens,
                row.counts.output_tokens,
            );
            if modern
                .range(row.at.saturating_sub(2000)..=row.at.saturating_add(2000))
                .flat_map(|(_, values)| values)
                .chain(modern.get(&i64::MIN).into_iter().flatten())
                .any(|usage| *usage == counts)
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
            parser.usage(&row.turn,&json!({"response_id":format!("legacy-{}-{}-{}",row.at,row.counts.input_tokens.unwrap_or(0),row.counts.output_tokens.unwrap_or(0)),"usage":usage}),Some(row.at));
            added = true;
        }
        if added {
            parser.warning("旧日志用量来自逐次快照校验，缺少请求计时");
        }
        if self.gaps && modern.is_empty() {
            parser.warning("部分旧快照缺少连续基线，未分摊累计用量");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
        parser.usage("modern",&json!({"response_id":"modern","usage":{"input_tokens":200,"cached_input_tokens":0,"output_tokens":20}}),Some(1781485380001));
        // Use the exact timestamp to model the dual records emitted by current clients.
        parser.turns.get_mut("modern").unwrap().turn.requests[0].settled_at =
            timestamp(&json!("2026-06-15T01:03:00.001Z"));
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
}
