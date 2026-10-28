use super::Record;
use crate::metrics::SpeedConfig;
use serde::Serialize;

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Generation {
    pub turn_id: String,
    pub phase: String,
    pub at: Option<i64>,
    pub config: SpeedConfig,
}

#[derive(Default)]
pub(super) struct State {
    pub value: Generation,
    target: String,
    owner: String,
    tier: Option<String>,
}

impl State {
    pub fn new(target: &str) -> Self {
        Self {
            target: target.into(),
            ..Default::default()
        }
    }
    pub fn uncertain(&mut self) {
        self.value.config = SpeedConfig::default();
    }
    pub fn owns(&self, record: &Record) -> bool {
        !self.target.is_empty()
            && record
                .payload
                .thread_id
                .as_ref()
                .map_or(self.owner == self.target, |id| id == &self.target)
    }
    pub fn apply(&mut self, record: &Record) {
        let p = &record.payload;
        if record.kind == "session_meta" {
            if p.id.as_deref() != Some(&self.owner) {
                self.tier = None;
            }
            self.owner = p.id.clone().unwrap_or_default();
            return;
        }
        if self.target.is_empty()
            || p.thread_id
                .as_ref()
                .map_or(self.owner != self.target, |id| id != &self.target)
        {
            return;
        }
        if p.thread_id.as_deref() == Some(&self.target) {
            self.owner = self.target.clone();
        }
        if record.kind == "event_msg" && p.kind == "thread_settings_applied" {
            self.tier = p
                .thread_settings
                .as_ref()
                .and_then(|v| v["service_tier"].as_str())
                .map(str::to_owned);
            self.value.config.service_tier = self.tier.clone();
            return;
        }
        if record.kind == "turn_context" {
            self.value.turn_id = p.turn_id.clone().unwrap_or_default();
            self.value.config = SpeedConfig {
                model: p.model.clone(),
                effort: p.effort.clone(),
                service_tier: self.tier.clone(),
            };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};
    fn apply(state: &mut State, kind: &str, payload: Value, second: u8) {
        state.apply(&serde_json::from_value(json!({"type":kind,"timestamp":format!("2026-10-28T00:00:{second:02}Z"),"payload":payload})).unwrap());
    }
    #[test]
    fn tools_and_late_lifecycle_events_do_not_replace_the_speed_configuration() {
        let mut state = State::new("main");
        apply(&mut state, "session_meta", json!({"id":"main"}), 0);
        apply(
            &mut state,
            "event_msg",
            json!({"type":"task_started","turn_id":"one"}),
            1,
        );
        apply(
            &mut state,
            "turn_context",
            json!({"turn_id":"one","model":"model","effort":"high"}),
            1,
        );
        for call in ["a", "b"] {
            apply(
                &mut state,
                "response_item",
                json!({"type":"function_call","call_id":call}),
                2,
            );
        }
        assert_eq!(state.value.config.model.as_deref(), Some("model"));
        apply(
            &mut state,
            "response_item",
            json!({"type":"function_call_output","call_id":"a","output":"{\"session_id\":123}"}),
            3,
        );
        assert_eq!(state.value.config.model.as_deref(), Some("model"));
        apply(
            &mut state,
            "event_msg",
            json!({"type":"item_completed","item":{"type":"CommandExecution","status":"completed"}}),
            4,
        );
        apply(
            &mut state,
            "response_item",
            json!({"type":"function_call_output","thread_id":"child","call_id":"b"}),
            4,
        );
        assert_eq!(state.value.config.model.as_deref(), Some("model"));
        apply(
            &mut state,
            "response_item",
            json!({"type":"function_call_output","call_id":"b","output":"Process running with session_id 42"}),
            5,
        );
        let config = state.value.config.clone();
        apply(
            &mut state,
            "response_item",
            json!({"type":"function_call_output","call_id":"unknown","output":"{\"type\":\"reasoning\"}"}),
            6,
        );
        assert_eq!(
            state.value.config, config,
            "tool text cannot alter model configuration"
        );
        apply(
            &mut state,
            "event_msg",
            json!({"type":"turn_aborted","turn_id":"one"}),
            7,
        );
        apply(&mut state, "response_item", json!({"type":"reasoning"}), 8);
        apply(
            &mut state,
            "event_msg",
            json!({"type":"task_started","turn_id":"one"}),
            8,
        );
        assert_eq!(
            state.value.config, config,
            "lifecycle is owned by the existing activity cursor"
        );
        apply(
            &mut state,
            "event_msg",
            json!({"type":"task_started","turn_id":"two"}),
            9,
        );
        apply(
            &mut state,
            "response_item",
            json!({"type":"function_call"}),
            10,
        );
        apply(&mut state, "response_item", json!({"type":"reasoning"}), 11);
        assert_eq!(state.value.config, config);
        apply(&mut state, "session_meta", json!({"id":"ancestor"}), 12);
        apply(
            &mut state,
            "event_msg",
            json!({"type":"task_started","turn_id":"inherited"}),
            13,
        );
        assert_eq!(
            state.value.turn_id, "one",
            "only own turn context changes the model configuration"
        );
    }
    #[test]
    fn tool_only_tail_cannot_invent_model_configuration() {
        let mut state = State::new("main");
        apply(&mut state, "session_meta", json!({"id":"main"}), 0);
        apply(
            &mut state,
            "response_item",
            json!({"type":"function_call_output","call_id":"missing-start"}),
            1,
        );
        assert!(state.value.config.model.is_none());
    }
    #[test]
    #[ignore = "replays own speed contexts and completed compactions without a live generation oracle"]
    fn live_generation_replay() {
        use std::io::{BufRead, BufReader, Read};
        let home = crate::source::codex_home().unwrap();
        let db = rusqlite::Connection::open_with_flags(
            home.join("state_5.sqlite"),
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .unwrap();
        for id in [
            "01a0ff60-165e-7ff1-89c5-4d58b94ce669",
            "01a162f3-1e03-7ff2-b959-44026ac0ff7b",
            "01a05327-af7d-7371-886d-7c0b06905ce4",
        ] {
            let path: String = db
                .query_row("SELECT rollout_path FROM threads WHERE id=?1", [id], |r| {
                    r.get(0)
                })
                .unwrap();
            let mut state = State::new(id);
            let (mut contexts, mut compactions) = (0, 0);
            for (path, end) in
                crate::preview::metrics_segments(&home, id, std::path::Path::new(&path)).unwrap()
            {
                let mut reader = BufReader::new(
                    crate::rollout_log::open(&path)
                        .unwrap()
                        .reader_at(0)
                        .unwrap()
                        .take(end),
                );
                let mut line = String::new();
                loop {
                    line.clear();
                    if reader.read_line(&mut line).unwrap() == 0 || !line.ends_with('\n') {
                        break;
                    }
                    let Ok(record) = serde_json::from_str::<Record>(&line) else {
                        continue;
                    };
                    let old = state.value.clone();
                    state.apply(&record);
                    if state.value.config != old.config {
                        contexts += 1;
                    }
                    if record.kind == "compacted" && state.owns(&record) {
                        compactions += 1;
                    }
                }
            }
            println!(
                "id={id} configuration_updates={contexts} completed_compactions={compactions}"
            );
        }
    }
}
