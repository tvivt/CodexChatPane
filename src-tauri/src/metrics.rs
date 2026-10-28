use rusqlite::{Connection, OpenFlags};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    io::{BufRead, BufReader, Read},
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

pub(crate) mod analysis;
pub(crate) mod details;
mod latest;
mod legacy;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Summary {
    pub input_tokens: Option<u64>,
    pub cached_input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub reasoning_output_tokens: Option<u64>,
    pub total_tokens: Option<u64>,
    pub request_count: usize,
    pub valid_request_count: usize,
    pub valid_output_tokens: u64,
    pub generation_ms: u64,
    pub token_speed: Option<f64>,
    pub cache_hit_rate: Option<f64>,
    pub coverage: Option<f64>,
    pub ttft_ms: Option<f64>,
    pub ttft_median_ms: Option<f64>,
    pub ttft_sample_count: usize,
    pub ttft_estimated: bool,
    pub duration_ms: Option<u64>,
    pub tool_wait_ms: u64,
    pub turn_count: usize,
    pub completed_turn_count: usize,
    #[serde(default)]
    pub activity_count: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Request {
    pub response_id: String,
    pub input_tokens: Option<u64>,
    pub cached_input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub reasoning_tokens: Option<u64>,
    pub start_ms: Option<i64>,
    pub end_ms: Option<i64>,
    pub included: bool,
    pub excluded_reason: Option<String>,
    pub settled_at: Option<i64>,
    pub cache_write_tokens: Option<u64>,
    pub cost_usd: Option<f64>,
    #[serde(skip)]
    total_tokens: Option<u64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Activity {
    pub id: String,
    pub r#type: String,
    pub label: String,
    pub at: Option<i64>,
    pub started_at: Option<i64>,
    pub completed_at: Option<i64>,
    pub status: Option<String>,
    pub detail: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Turn {
    pub turn_id: String,
    pub model: Option<String>,
    pub effort: Option<String>,
    pub service_tier: Option<String>,
    pub status: String,
    pub started_at: Option<i64>,
    pub completed_at: Option<i64>,
    pub metrics: Summary,
    pub requests: Vec<Request>,
    pub activities: Vec<Activity>,
    pub consumption: Option<crate::usage::costs::Totals>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Tool {
    pub name: String,
    pub count: usize,
    pub failures: usize,
    pub runtime_ms: u64,
    pub wait_ms: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Subagent {
    pub thread_id: String,
    pub name: String,
    pub path: Option<String>,
    pub model: Option<String>,
    pub status: String,
    pub summary: Option<Summary>,
    pub error: Option<String>,
    pub consumption: Option<crate::usage::costs::Consumption>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatMetrics {
    pub thread_id: String,
    pub title: String,
    pub cwd: Option<String>,
    pub model: Option<String>,
    pub effort: Option<String>,
    pub archived: bool,
    pub created_at: Option<i64>,
    pub updated_at: Option<i64>,
    pub as_of: i64,
    pub complete: bool,
    pub warnings: Vec<String>,
    pub latest_turn_id: Option<String>,
    pub summary: Summary,
    pub turns: Vec<Turn>,
    pub tools: Vec<Tool>,
    pub subagents: Vec<Subagent>,
    pub consumption: Option<crate::usage::costs::Consumption>,
    #[serde(default)]
    pub recent_speed: Option<SpeedEstimate>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail_revision: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turn_offset: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turn_total: Option<usize>,
    #[serde(skip)]
    pub(crate) parent_thread_id: Option<String>,
    #[serde(skip)]
    pub(crate) usage_batches: Vec<(Option<String>, Option<String>, Vec<Request>)>,
    #[serde(skip)]
    pub(crate) usage_complete: bool,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SpeedConfig {
    pub model: Option<String>,
    pub effort: Option<String>,
    pub service_tier: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SpeedEstimate {
    pub config: SpeedConfig,
    pub token_speed: f64,
    pub samples: usize,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
struct Usage {
    input_tokens: Option<u64>,
    cached_input_tokens: Option<u64>,
    output_tokens: Option<u64>,
    reasoning_output_tokens: Option<u64>,
    total_tokens: Option<u64>,
}

#[derive(Clone, Default, Serialize, Deserialize)]
struct Candidate {
    starts: Vec<i64>,
    reasoning_start: bool,
    ends: Vec<i64>,
    outputs: Vec<i64>,
    broken: bool,
}

#[derive(Clone, Serialize, Deserialize)]
struct TurnState {
    turn: Turn,
    candidate: Candidate,
    boundary: Option<i64>,
    own: bool,
    fallback: Option<Usage>,
    native_ttft: Option<f64>,
    ttft_started_at: Option<i64>,
    first_text_at: Option<i64>,
    duration: Option<u64>,
}
impl TurnState {
    fn new(id: &str) -> Self {
        Self {
            turn: Turn {
                turn_id: id.into(),
                model: None,
                effort: None,
                service_tier: None,
                status: "inProgress".into(),
                started_at: None,
                completed_at: None,
                metrics: Summary::default(),
                requests: vec![],
                activities: vec![],
                consumption: None,
            },
            candidate: Candidate::default(),
            boundary: None,
            own: false,
            fallback: None,
            native_ttft: None,
            ttft_started_at: None,
            first_text_at: None,
            duration: None,
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
struct Call {
    turn: String,
    name: String,
    start: i64,
    end: Option<i64>,
    failure: bool,
    runtime: u64,
}
#[derive(Clone, Serialize, Deserialize)]
struct Parser {
    id: String,
    active_thread: String,
    active_turn: Option<String>,
    service_tier: Option<String>,
    turns: BTreeMap<String, TurnState>,
    order: Vec<String>,
    usages: HashMap<String, (Usage, String, usize)>,
    calls: HashMap<String, Call>,
    tools: BTreeMap<String, Tool>,
    children: BTreeMap<String, Subagent>,
    warnings: Vec<String>,
    complete: bool,
    item_ids: HashSet<String>,
}

impl Parser {
    fn new(id: &str) -> Self {
        Self {
            id: id.into(),
            active_thread: id.into(),
            active_turn: None,
            service_tier: None,
            turns: BTreeMap::new(),
            order: vec![],
            usages: HashMap::new(),
            calls: HashMap::new(),
            tools: BTreeMap::new(),
            children: BTreeMap::new(),
            warnings: vec![],
            complete: true,
            item_ids: HashSet::new(),
        }
    }
    fn warning(&mut self, message: &str) {
        self.complete = false;
        if !self.warnings.iter().any(|value| value == message) {
            self.warnings.push(message.into());
        }
    }
    fn turn(&mut self, id: &str) -> &mut TurnState {
        if !self.turns.contains_key(id) {
            self.order.push(id.into());
            let mut state = TurnState::new(id);
            state.turn.service_tier = self.service_tier.clone();
            self.turns.insert(id.into(), state);
        }
        self.turns.get_mut(id).unwrap()
    }
    fn bad_line(&mut self) {
        self.warning("日志包含坏行或未完成尾行");
        if let Some(id) = self.active_turn.clone() {
            self.turn(&id).candidate.broken = true;
        }
    }
    fn apply(&mut self, record: Value) {
        let p = &record["payload"];
        let kind = text(&record["type"]);
        let at = timestamp(&record["timestamp"]);
        if kind == "session_meta" {
            if text(&p["id"]) != self.active_thread {
                self.service_tier = None;
            }
            self.active_thread = text(&p["id"]).into();
            self.active_turn = None;
            return;
        }
        let explicit_thread = p["thread_id"].as_str();
        if explicit_thread.is_some_and(|id| id != self.id) {
            return;
        }
        // Forked logs copy a parent's session_meta; the child's settings event starts its own history.
        if explicit_thread == Some(self.id.as_str()) {
            self.active_thread = self.id.clone();
        }
        if explicit_thread.is_none() && self.active_thread != self.id {
            return;
        }
        let event = text(&p["type"]);
        if kind == "event_msg" && event == "thread_settings_applied" {
            self.service_tier = p["thread_settings"]["service_tier"]
                .as_str()
                .map(str::to_owned);
            return;
        }
        let turn_id = p["turn_id"]
            .as_str()
            .map(str::to_owned)
            .or_else(|| {
                p["internal_chat_message_metadata_passthrough"]["turn_id"]
                    .as_str()
                    .map(str::to_owned)
            })
            .or_else(|| self.active_turn.clone());
        if matches!(kind, "turn_context") || kind == "event_msg" && event == "task_started" {
            if let Some(id) = &turn_id {
                self.active_turn = Some(id.clone());
            }
        }
        let Some(id) = turn_id else {
            return;
        };
        let own = explicit_thread == Some(self.id.as_str()) || self.active_thread == self.id;
        if own {
            self.turn(&id).own = true;
        }
        match (kind, event) {
            ("turn_context", _) => {
                let state = self.turn(&id);
                state.turn.model = p["model"].as_str().map(str::to_owned);
                state.turn.effort = p["effort"].as_str().map(str::to_owned);
            }
            ("event_msg", "task_started") => {
                let state = self.turn(&id);
                state.turn.started_at = time_field(p, "started_at").or(at);
                state.ttft_started_at = at.or(state.turn.started_at);
            }
            ("event_msg", "task_complete") => {
                let state = self.turn(&id);
                state.turn.status = "completed".into();
                state.turn.started_at = time_field(p, "started_at").or(state.turn.started_at);
                state.turn.completed_at = time_field(p, "completed_at").or(at);
                state.native_ttft = p["time_to_first_token_ms"].as_f64().filter(|n| *n >= 0.0);
                state.duration = p["duration_ms"].as_u64();
            }
            ("event_msg", "turn_aborted") => {
                let state = self.turn(&id);
                state.turn.status = "interrupted".into();
                state.turn.completed_at = time_field(p, "completed_at").or(at);
                state.duration = p["duration_ms"].as_u64();
                state.native_ttft = p["time_to_first_token_ms"].as_f64().filter(|n| *n >= 0.0);
            }
            ("event_msg", "item_completed") => self.item(&id, p, at),
            ("event_msg", "token_count") => {
                if let Ok(usage) =
                    serde_json::from_value::<Usage>(p["info"]["total_token_usage"].clone())
                {
                    self.turn(&id).fallback = Some(usage);
                }
            }
            ("token_usage_record", _) => self.usage(&id, p, at),
            ("response_item", "function_call" | "custom_tool_call") => {
                let call_id = text(&p["call_id"]);
                if let Some(at) = at {
                    self.turn(&id).candidate.ends.push(at);
                    self.turn(&id).candidate.outputs.push(at);
                    if !call_id.is_empty() && !self.calls.contains_key(call_id) {
                        let name = if text(&p["namespace"]).is_empty() {
                            text(&p["name"]).into()
                        } else {
                            format!("{}.{}", text(&p["namespace"]), text(&p["name"]))
                        };
                        self.calls.insert(
                            call_id.into(),
                            Call {
                                turn: id.clone(),
                                name: name.clone(),
                                start: at,
                                end: None,
                                failure: false,
                                runtime: 0,
                            },
                        );
                        self.tools
                            .entry(name.clone())
                            .or_insert_with(|| Tool {
                                name,
                                ..Default::default()
                            })
                            .count += 1;
                        self.turn(&id).turn.activities.push(Activity {
                            id: call_id.into(),
                            r#type: "ToolCall".into(),
                            label: text(&p["name"]).into(),
                            at: Some(at),
                            started_at: Some(at),
                            completed_at: None,
                            status: None,
                            detail: argument_detail(p),
                        });
                    }
                }
            }
            ("response_item", "function_call_output" | "custom_tool_call_output") => {
                let call_id = text(&p["call_id"]);
                if let Some(call) = self.calls.get_mut(call_id) {
                    if call.end.is_none() {
                        call.end = at;
                        call.failure |= p["output"]["isError"].as_bool() == Some(true);
                    }
                }
                if let Some(activity) = self
                    .turn(&id)
                    .turn
                    .activities
                    .iter_mut()
                    .find(|a| a.id == call_id && a.r#type == "ToolCall")
                {
                    activity.completed_at = at;
                    activity.status = Some("returned".into());
                }
            }
            _ => {}
        }
    }

    fn item(&mut self, turn: &str, p: &Value, at: Option<i64>) {
        let item = &p["item"];
        let kind = text(&item["type"]);
        let id = text(&item["id"]);
        if !id.is_empty() && !self.item_ids.insert(format!("{turn}:{kind}:{id}")) {
            return;
        }
        let start = p["started_at_ms"].as_i64();
        let end = p["completed_at_ms"].as_i64();
        if kind == "AgentMessage" {
            if let Some(start) = start.filter(|start| end.is_some_and(|end| end > *start)) {
                let state = self.turn(turn);
                state.first_text_at =
                    Some(state.first_text_at.map_or(start, |first| first.min(start)));
            }
        }
        if matches!(kind, "Reasoning" | "AgentMessage") {
            let candidate = &mut self.turn(turn).candidate;
            if let Some(end) = end {
                candidate.ends.push(end);
                candidate.outputs.push(start.unwrap_or(end));
            }
            if let (Some(start), Some(end)) = (start, end) {
                if end > start {
                    candidate.starts.push(start);
                    candidate.reasoning_start |= kind == "Reasoning";
                }
            }
        }
        if kind == "SubAgentActivity" {
            let child = text(&item["agent_thread_id"]);
            let path = item["agent_path"].as_str().map(str::to_owned);
            if !child.is_empty() {
                let name = path
                    .as_deref()
                    .unwrap_or(child)
                    .rsplit('/')
                    .next()
                    .unwrap_or(child)
                    .into();
                let entry = self
                    .children
                    .entry(child.into())
                    .or_insert_with(|| Subagent {
                        thread_id: child.into(),
                        name,
                        path,
                        model: None,
                        status: "unknown".into(),
                        summary: None,
                        error: None,
                        consumption: None,
                    });
                entry.status = text(&item["kind"]).into();
            }
        }
        let failure = text(&item["status"]) == "failed"
            || item["exit_code"].as_i64().is_some_and(|code| code != 0);
        let runtime = match (start, end) {
            (Some(s), Some(e)) if e >= s => (e - s) as u64,
            _ => 0,
        };
        if matches!(
            kind,
            "McpToolCall" | "CommandExecution" | "CollabAgentToolCall" | "FileChange" | "Sleep"
        ) {
            if let Some(call) = self.calls.get_mut(id) {
                call.runtime = call.runtime.max(runtime);
                call.failure |= failure;
            } else {
                // ponytail: nested command runtimes remain per-tool rows; never add them to global blocking time.
                let name = match kind {
                    "McpToolCall" => format!("{}.{}", text(&item["server"]), text(&item["tool"])),
                    "CollabAgentToolCall" => format!("collaboration.{}", text(&item["tool"])),
                    _ => kind.into(),
                };
                let tool = self.tools.entry(name.clone()).or_insert_with(|| Tool {
                    name,
                    ..Default::default()
                });
                tool.count += 1;
                tool.runtime_ms += runtime;
                tool.failures += usize::from(failure);
            }
        }
        if matches!(
            kind,
            "Reasoning"
                | "AgentMessage"
                | "McpToolCall"
                | "CommandExecution"
                | "CollabAgentToolCall"
                | "FileChange"
                | "SubAgentActivity"
                | "ContextCompaction"
                | "Sleep"
        ) {
            let label = match kind {
                "McpToolCall" => format!("{}.{}", text(&item["server"]), text(&item["tool"])),
                "CollabAgentToolCall" => format!("collaboration.{}", text(&item["tool"])),
                "SubAgentActivity" => item["agent_path"].as_str().unwrap_or(kind).into(),
                _ => kind.into(),
            };
            self.turn(turn).turn.activities.push(Activity {
                id: id.into(),
                r#type: kind.into(),
                label,
                at,
                started_at: start,
                completed_at: end,
                status: item["status"].as_str().map(str::to_owned),
                detail: item_detail(item),
            });
        }
    }

    fn usage(&mut self, turn: &str, p: &Value, at: Option<i64>) {
        let response = text(&p["response_id"]);
        if response.is_empty() {
            self.warning("逐请求用量缺少 response_id");
            self.turn(turn).candidate.broken = true;
            return;
        }
        let Ok(usage) = serde_json::from_value::<Usage>(p["usage"].clone()) else {
            self.warning("逐请求用量格式无效");
            self.turn(turn).candidate.broken = true;
            return;
        };
        if let Some((old, old_turn, index)) = self.usages.get(response).cloned() {
            if old != usage || old_turn != turn {
                self.warning("重复请求用量冲突");
                if let Some(request) = self.turn(&old_turn).turn.requests.get_mut(index) {
                    exclude(request, "用量冲突");
                }
                self.turn(turn).candidate.broken = true;
            }
            return;
        }
        let state = self.turn(turn);
        state.own = true;
        let candidate = std::mem::take(&mut state.candidate);
        let start = candidate.starts.iter().min().copied();
        let end = candidate.ends.iter().max().copied();
        let mut reason = None;
        if candidate.broken {
            reason = Some("日志段不完整");
        } else if usage.input_tokens.is_none()
            || usage.output_tokens.is_none()
            || usage.cached_input_tokens.is_none()
            || usage.cached_input_tokens > usage.input_tokens
            || usage.reasoning_output_tokens > usage.output_tokens
        {
            reason = Some("用量字段无效");
        } else if start.is_none() {
            reason = Some("缺少输出起点");
        } else if usage.reasoning_output_tokens.unwrap_or(0) > 0 && !candidate.reasoning_start {
            reason = Some("缺少推理计时");
        } else if candidate.outputs.iter().any(|time| Some(*time) < start) {
            reason = Some("输出前段缺失");
        } else if end <= start
            || usage.output_tokens.unwrap_or(0) <= 1
            || at.is_some_and(|a| end.is_some_and(|e| e > a))
        {
            reason = Some("生成时间无效");
        } else if start.is_some_and(|s| state.boundary.is_some_and(|b| s < b)) {
            reason = Some("跨越前一请求");
        }
        let request = Request {
            response_id: response.into(),
            input_tokens: usage.input_tokens,
            cached_input_tokens: usage.cached_input_tokens,
            output_tokens: usage.output_tokens,
            reasoning_tokens: usage.reasoning_output_tokens,
            total_tokens: usage.total_tokens,
            start_ms: start,
            end_ms: end,
            included: reason.is_none(),
            excluded_reason: reason.map(str::to_owned),
            settled_at: at,
            cache_write_tokens: p["usage"]["cache_write_tokens"]
                .as_u64()
                .or_else(|| p["usage"]["input_tokens_details"]["cache_write_tokens"].as_u64()),
            cost_usd: None,
        };
        let index = state.turn.requests.len();
        state.turn.requests.push(request);
        state.boundary = at.or(end);
        self.usages
            .insert(response.into(), (usage, turn.into(), index));
    }

    fn finish(mut self) -> (Vec<Turn>, Vec<Tool>, Vec<Subagent>, Vec<String>, bool) {
        let mut waits: HashMap<String, Vec<(i64, i64)>> = HashMap::new();
        let mut missing = HashMap::<String, Vec<i64>>::new();
        for call in self.calls.values() {
            let tool = self.tools.get_mut(&call.name).unwrap();
            tool.runtime_ms += call.runtime;
            tool.failures += usize::from(call.failure);
            if let Some(end) = call.end.filter(|end| *end >= call.start) {
                tool.wait_ms += (end - call.start) as u64;
                waits
                    .entry(call.turn.clone())
                    .or_default()
                    .push((call.start, end));
            } else {
                missing
                    .entry(call.turn.clone())
                    .or_default()
                    .push(call.start);
            }
        }
        if !missing.is_empty() {
            self.warning("部分工具缺少返回记录，相关速度样本已排除");
        }
        let mut previous_end = None;
        let mut turns = vec![];
        for id in self.order.clone() {
            let Some(mut state) = self.turns.remove(&id).filter(|state| state.own) else {
                continue;
            };
            let intervals = waits.get(&id).cloned().unwrap_or_default();
            for request in &mut state.turn.requests {
                if request.included {
                    let start = request.start_ms.unwrap();
                    let end = request.end_ms.unwrap();
                    if intervals.iter().any(|&(s, e)| start < e && end > s)
                        || missing
                            .get(&id)
                            .is_some_and(|items| items.iter().any(|s| end > *s))
                    {
                        exclude(request, "工具等待重叠");
                    } else if previous_end.is_some_and(|e| start < e) {
                        exclude(request, "请求区间重叠");
                    }
                    if request.included {
                        previous_end = Some(end);
                    }
                }
            }
            let mut metrics = summarize(&state.turn.requests);
            // Tool-first turns lack a reliable first-token boundary before the first text.
            let estimated_ttft = state
                .ttft_started_at
                .zip(state.first_text_at)
                .filter(|(start, first)| {
                    first >= start
                        && !intervals.iter().any(|&(tool, _)| tool < *first)
                        && !missing
                            .get(&id)
                            .is_some_and(|tools| tools.iter().any(|tool| *tool < *first))
                })
                .map(|(start, first)| (first - start) as f64);
            metrics.ttft_ms = state.native_ttft.or(estimated_ttft);
            metrics.ttft_estimated = state.native_ttft.is_none() && metrics.ttft_ms.is_some();
            metrics.ttft_median_ms = metrics.ttft_ms;
            metrics.ttft_sample_count = usize::from(metrics.ttft_ms.is_some());
            metrics.duration_ms =
                state
                    .duration
                    .or_else(|| match (state.turn.started_at, state.turn.completed_at) {
                        (Some(s), Some(e)) if e >= s => Some((e - s) as u64),
                        _ => None,
                    });
            metrics.tool_wait_ms = interval_sum(intervals);
            metrics.turn_count = 1;
            metrics.completed_turn_count = usize::from(state.turn.status == "completed");
            metrics.activity_count = state.turn.activities.len();
            // Old token_count is a thread cumulative snapshot, never add snapshots from separate turns.
            if state.turn.requests.is_empty() && state.fallback.is_some() {
                self.warning("旧日志仅有线程累计用量；轮次用量与生成速度不可用");
            }
            state.turn.metrics = metrics;
            turns.push(state.turn);
        }
        (
            turns,
            self.tools.into_values().collect(),
            self.children.into_values().collect(),
            self.warnings,
            self.complete,
        )
    }
}

fn exclude(request: &mut Request, reason: &str) {
    request.included = false;
    request.excluded_reason = Some(reason.into());
}
fn text(value: &Value) -> &str {
    value.as_str().unwrap_or("")
}
fn timestamp(value: &Value) -> Option<i64> {
    value
        .as_str()
        .and_then(|s| {
            time::OffsetDateTime::parse(s, &time::format_description::well_known::Rfc3339).ok()
        })
        .map(|date| (date.unix_timestamp_nanos() / 1_000_000) as i64)
}
fn time_field(value: &Value, key: &str) -> Option<i64> {
    value[key].as_i64().map(|n| {
        if n.abs() > 10_000_000_000 {
            n
        } else {
            n.saturating_mul(1000)
        }
    })
}
fn short(value: String) -> Option<String> {
    (!value.is_empty()).then(|| value.chars().take(4000).collect())
}
fn item_detail(item: &Value) -> Option<String> {
    match text(&item["type"]) {
        "CommandExecution" => {
            let command = item["command"]
                .as_array()
                .map(|args| {
                    args.iter()
                        .filter_map(Value::as_str)
                        .collect::<Vec<_>>()
                        .join(" ")
                })
                .unwrap_or_else(|| text(&item["command"]).into());
            short(format!(
                "{}\nexit_code: {}\n{}",
                command,
                item["exit_code"],
                text(&item["aggregated_output"])
            ))
        }
        "FileChange" => item["changes"]
            .as_object()
            .and_then(|changes| short(changes.keys().cloned().collect::<Vec<_>>().join("\n"))),
        "AgentMessage" => item["content"].as_array().and_then(|parts| {
            short(
                parts
                    .iter()
                    .filter_map(|part| part["text"].as_str())
                    .collect::<Vec<_>>()
                    .join("\n"),
            )
        }),
        "SubAgentActivity" => short(text(&item["kind"]).into()),
        "McpToolCall" => argument_detail(item),
        _ => None,
    }
}
fn argument_detail(item: &Value) -> Option<String> {
    let args = if item["arguments"].is_object() {
        item["arguments"].clone()
    } else {
        serde_json::from_str::<Value>(text(&item["arguments"])).ok()?
    };
    let mut parts = [
        "path",
        "file_path",
        "query",
        "cmd",
        "command",
        "pattern",
        "target",
    ]
    .into_iter()
    .filter_map(|key| args[key].as_str().map(|value| format!("{key}: {value}")))
    .collect::<Vec<_>>();
    if let Some(files) = args["files"].as_array() {
        parts.extend(
            files
                .iter()
                .filter_map(|file| file["path"].as_str())
                .map(|path| format!("path: {path}")),
        );
    }
    short(parts.join("\n"))
}
fn interval_sum(mut intervals: Vec<(i64, i64)>) -> u64 {
    intervals.sort_unstable();
    let mut sum = 0u64;
    let mut last: Option<(i64, i64)> = None;
    for (start, end) in intervals {
        match last {
            Some((s, e)) if start <= e => last = Some((s, e.max(end))),
            Some((s, e)) => {
                sum += (e - s).max(0) as u64;
                last = Some((start, end));
            }
            None => last = Some((start, end)),
        }
    }
    if let Some((s, e)) = last {
        sum += (e - s).max(0) as u64;
    }
    sum
}
fn sum_options<'a>(values: impl Iterator<Item = Option<u64>>) -> Option<u64> {
    let values: Vec<_> = values.collect();
    if values.is_empty() {
        return None;
    }
    values
        .into_iter()
        .try_fold(0u64, |sum, n| sum.checked_add(n?))
}
fn summarize(requests: &[Request]) -> Summary {
    let mut result = Summary {
        input_tokens: sum_options(requests.iter().map(|r| r.input_tokens)),
        cached_input_tokens: sum_options(requests.iter().map(|r| r.cached_input_tokens)),
        output_tokens: sum_options(requests.iter().map(|r| r.output_tokens)),
        reasoning_output_tokens: sum_options(requests.iter().map(|r| r.reasoning_tokens)),
        total_tokens: sum_options(requests.iter().map(|r| r.total_tokens)),
        request_count: requests.len(),
        ..Default::default()
    };
    for request in requests.iter().filter(|r| r.included) {
        result.valid_request_count += 1;
        result.valid_output_tokens += request.output_tokens.unwrap_or(0);
        result.generation_ms += (request.end_ms.unwrap() - request.start_ms.unwrap()) as u64;
    }
    result.token_speed = (result.generation_ms > 0).then(|| {
        (result.valid_output_tokens - result.valid_request_count as u64) as f64 * 1000.0
            / result.generation_ms as f64
    });
    result.cache_hit_rate = match (result.input_tokens, result.cached_input_tokens) {
        (Some(input), Some(cached)) if input > 0 && cached <= input => {
            Some(cached as f64 / input as f64)
        }
        _ => None,
    };
    result.coverage = result
        .output_tokens
        .filter(|output| *output > 0)
        .map(|output| result.valid_output_tokens as f64 / output as f64);
    result
}
fn aggregate(turns: &[Turn]) -> Summary {
    let completed: Vec<_> = turns
        .iter()
        .filter(|turn| turn.status == "completed")
        .collect();
    let requests: Vec<_> = completed
        .iter()
        .flat_map(|turn| turn.requests.iter().cloned())
        .collect();
    let mut summary = summarize(&requests);
    summary.turn_count = turns.len();
    summary.completed_turn_count = completed.len();
    summary.activity_count = completed
        .iter()
        .map(|turn| turn.metrics.activity_count)
        .sum();
    summary.duration_ms = sum_options(completed.iter().map(|turn| turn.metrics.duration_ms));
    summary.tool_wait_ms = completed.iter().map(|turn| turn.metrics.tool_wait_ms).sum();
    let mut ttfts: Vec<_> = completed
        .iter()
        .filter_map(|turn| turn.metrics.ttft_ms)
        .collect();
    ttfts.sort_by(f64::total_cmp);
    summary.ttft_sample_count = ttfts.len();
    summary.ttft_estimated = completed.iter().any(|turn| turn.metrics.ttft_estimated);
    if !ttfts.is_empty() {
        summary.ttft_ms = Some(ttfts.iter().sum::<f64>() / ttfts.len() as f64);
        summary.ttft_median_ms =
            Some((ttfts[(ttfts.len() - 1) / 2] + ttfts[ttfts.len() / 2]) / 2.0);
    }
    summary
}

pub fn read_latest(thread_id: &str) -> Result<ChatMetrics, String> {
    latest::read(&crate::source::codex_home()?, thread_id)
}

pub(crate) fn read_own(home: &Path, thread_id: &str) -> Result<ChatMetrics, String> {
    analysis::read(home, thread_id).map(|(data, _)| data)
}

fn catalog(home: &Path, thread_id: &str) -> Result<(String, ChatMetrics), String> {
    let db = Connection::open_with_flags(
        home.join("state_5.sqlite"),
        OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .map_err(|e| e.to_string())?;
    let title = crate::source::thread_title_expression(&db)?;
    let (path,title,cwd,model,archived,created,updated): (String,String,Option<String>,Option<String>,bool,Option<i64>,Option<i64>) = db.query_row(
        &format!("SELECT rollout_path,{title},cwd,model,archived,created_at,updated_at FROM threads WHERE id=?1"), [thread_id],
        |row|Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?,row.get(4)?,row.get(5)?,row.get(6)?))).map_err(|e|e.to_string())?;
    let as_of = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64;
    Ok((
        path,
        ChatMetrics {
            thread_id: thread_id.into(),
            title,
            cwd,
            model,
            effort: None,
            archived,
            created_at: created.map(|n| n.saturating_mul(1000)),
            updated_at: updated.map(|n| n.saturating_mul(1000)),
            as_of,
            complete: true,
            warnings: vec![],
            parent_thread_id: None,
            usage_batches: vec![],
            usage_complete: true,
            latest_turn_id: None,
            summary: Summary::default(),
            turns: vec![],
            tools: vec![],
            subagents: vec![],
            consumption: None,
            recent_speed: None,
            detail_revision: None,
            turn_offset: None,
            turn_total: None,
        },
    ))
}

fn read_at(home: &Path, thread_id: &str, children: bool) -> Result<ChatMetrics, String> {
    read_at_mode(home, thread_id, children, false)
}

fn read_at_mode(
    home: &Path,
    thread_id: &str,
    children: bool,
    usage_only: bool,
) -> Result<ChatMetrics, String> {
    let (path, mut data) = catalog(home, thread_id)?;
    let segments = crate::preview::metrics_segments(home, thread_id, Path::new(&path))?;
    let mut parser = Parser::new(thread_id);
    let mut legacy = legacy::Collector::new(thread_id);
    let mut last_fallback = None;
    let mut subagent = false;
    let mut inherited_history = false;
    let mut parent_thread_id = None;
    for (path, end) in segments {
        let file = crate::rollout_log::open(&path)
            .and_then(|log| log.reader_at(0))
            .map_err(|e| e.to_string())?;
        let mut reader = BufReader::new(file.take(end));
        let mut line = String::new();
        loop {
            line.clear();
            if reader.read_line(&mut line).map_err(|e| e.to_string())? == 0 {
                break;
            }
            if !line.ends_with('\n') {
                parser.bad_line();
                break;
            }
            match serde_json::from_str::<Value>(&line) {
                Ok(record) => {
                    legacy.apply(&record);
                    if usage_only
                        && !matches!(
                            record["type"].as_str(),
                            Some("session_meta" | "turn_context" | "token_usage_record")
                        )
                        && !(record["type"] == "event_msg"
                            && matches!(
                                record["payload"]["type"].as_str(),
                                Some(
                                    "task_started"
                                        | "task_complete"
                                        | "turn_aborted"
                                        | "thread_settings_applied"
                                )
                            ))
                    {
                        continue;
                    }
                    if record["type"] == "session_meta" && record["payload"]["id"] != thread_id {
                        inherited_history = true;
                    }
                    if record["type"] == "session_meta" && record["payload"]["id"] == thread_id {
                        subagent |= record["payload"]["thread_source"] == "subagent";
                        parent_thread_id = record["payload"]["parent_thread_id"]
                            .as_str()
                            .map(str::to_owned)
                            .or_else(|| {
                                record["payload"]["source"]["subagent"]["thread_spawn"]
                                    ["parent_thread_id"]
                                    .as_str()
                                    .map(str::to_owned)
                            });
                    }
                    if parser.active_thread == thread_id
                        && record["type"] == "event_msg"
                        && record["payload"]["type"] == "token_count"
                        && record["payload"]["thread_id"]
                            .as_str()
                            .is_none_or(|id| id == thread_id)
                    {
                        last_fallback = serde_json::from_value::<Usage>(
                            record["payload"]["info"]["total_token_usage"].clone(),
                        )
                        .ok();
                    }
                    parser.apply(record);
                }
                Err(_) => parser.bad_line(),
            }
        }
    }
    legacy.attach(&mut parser);
    let (turns, tools, mut subagents, mut warnings, mut complete) = parser.finish();
    let mut summary = aggregate(&turns);
    if turns.iter().all(|turn| turn.requests.is_empty()) && !subagent && !inherited_history {
        if let Some(usage) = last_fallback {
            summary.input_tokens = usage.input_tokens;
            summary.cached_input_tokens = usage.cached_input_tokens;
            summary.output_tokens = usage.output_tokens;
            summary.reasoning_output_tokens = usage.reasoning_output_tokens;
            summary.total_tokens = usage.total_tokens;
            summary.cache_hit_rate = match (usage.input_tokens, usage.cached_input_tokens) {
                (Some(n), Some(c)) if n > 0 && c <= n => Some(c as f64 / n as f64),
                _ => None,
            };
            complete = false;
            warnings.push("用量来自旧版线程累计快照，可能包含继承历史".into());
        }
    }
    let latest_turn_id = turns
        .iter()
        .max_by_key(|turn| turn.started_at.unwrap_or(0))
        .map(|turn| turn.turn_id.clone());
    let effort = turns.last().and_then(|turn| turn.effort.clone());
    if children {
        subagents.retain_mut(|agent| {
            match catalog(home, &agent.thread_id) {
                Ok((_, result)) => {
                    agent.model = result.model;
                }
                Err(error) => {
                    agent.error = Some(error);
                    agent.status = "unavailable".into();
                }
            }
            true
        });
    } else {
        subagents.clear();
    }
    data.effort = effort;
    data.complete = complete;
    data.warnings = warnings;
    data.parent_thread_id = parent_thread_id;
    data.latest_turn_id = latest_turn_id;
    data.summary = summary;
    data.turns = turns;
    data.tools = tools;
    data.subagents = subagents;
    if !usage_only {
        crate::usage::costs::attach_metrics(&mut data);
    }
    Ok(data)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn event(parser: &mut Parser, payload: Value) {
        parser.apply(json!({"type":"event_msg","payload":payload}));
    }
    fn output(parser: &mut Parser, kind: &str, id: &str, start: i64, end: i64) {
        event(
            parser,
            json!({"type":"item_completed","thread_id":"main","turn_id":"turn","started_at_ms":start,"completed_at_ms":end,
            "item":{"type":kind,"id":id}}),
        );
    }
    fn usage(parser: &mut Parser, response: &str, output: u64, reasoning: u64, at: &str) {
        parser.apply(json!({"type":"token_usage_record","timestamp":at,"payload":{"thread_id":"main","turn_id":"turn","response_id":response,
            "usage":{"input_tokens":100,"cached_input_tokens":90,"output_tokens":output,"reasoning_output_tokens":reasoning,"total_tokens":100+output}}}));
    }
    fn complete(parser: &mut Parser) {
        event(
            parser,
            json!({"type":"task_complete","turn_id":"turn","duration_ms":65000,"time_to_first_token_ms":1800}),
        );
    }

    #[test]
    fn service_tier_is_captured_per_turn_without_relabeling_history() {
        let mut parser = Parser::new("main");
        let start = |parser: &mut Parser, id: &str| {
            event(parser, json!({"type":"task_started","turn_id":id}))
        };
        let settings = |parser: &mut Parser, id: &str, value: Value| {
            event(
                parser,
                json!({"type":"thread_settings_applied","thread_id":id,"thread_settings":value}),
            )
        };
        start(&mut parser, "unknown");
        settings(&mut parser, "main", json!({"service_tier":"priority"}));
        start(&mut parser, "fast");
        settings(&mut parser, "main", json!({"service_tier":"default"}));
        parser.apply(json!({"type":"turn_context","payload":{"turn_id":"fast","model":"model"}}));
        start(&mut parser, "standard");
        settings(&mut parser, "child", json!({"service_tier":"priority"}));
        start(&mut parser, "still-standard");
        settings(&mut parser, "main", json!({}));
        start(&mut parser, "missing");
        settings(&mut parser, "main", json!({"service_tier":"fast"}));
        start(&mut parser, "fast-alias");
        parser.apply(json!({"type":"session_meta","payload":{"id":"main"}}));
        start(&mut parser, "same-thread-continuation");
        parser.apply(json!({"type":"session_meta","payload":{"id":"parent"}}));
        settings(&mut parser, "parent", json!({"service_tier":"priority"}));
        start(&mut parser, "inherited");
        settings(&mut parser, "main", json!({"service_tier":null}));
        start(&mut parser, "fork-unknown");
        let (turns, _, _, _, _) = parser.finish();
        let tiers: HashMap<_, _> = turns
            .iter()
            .map(|turn| (turn.turn_id.as_str(), turn.service_tier.as_deref()))
            .collect();
        assert_eq!(tiers["unknown"], None);
        assert_eq!(tiers["fast"], Some("priority"));
        assert_eq!(tiers["standard"], Some("default"));
        assert_eq!(tiers["still-standard"], Some("default"));
        assert_eq!(tiers["missing"], None);
        assert_eq!(tiers["fast-alias"], Some("fast"));
        assert_eq!(tiers["same-thread-continuation"], Some("fast"));
        assert_eq!(tiers["fork-unknown"], None);
        assert!(!tiers.contains_key("inherited"));
        assert_eq!(
            serde_json::to_value(&turns[1]).unwrap()["serviceTier"],
            "priority"
        );
    }

    #[test]
    fn weighted_speed_keeps_tokens_and_tool_parameter_end_excludes_tool_wait() {
        let mut parser = Parser::new("main");
        parser.active_turn = Some("turn".into());
        output(&mut parser, "Reasoning", "r1", 1000, 1900);
        output(&mut parser, "AgentMessage", "a1", 1900, 3000);
        usage(&mut parser, "one", 101, 20, "1970-01-01T00:00:03.010Z");
        usage(&mut parser, "one", 101, 20, "1970-01-01T00:00:03.010Z");
        output(&mut parser, "Reasoning", "r2", 4000, 5500);
        output(&mut parser, "AgentMessage", "a2", 5500, 6000);
        parser.apply(json!({"type":"response_item","timestamp":"1970-01-01T00:00:07Z","payload":{"type":"function_call","call_id":"call","name":"tool"}}));
        usage(&mut parser, "two", 301, 30, "1970-01-01T00:00:07.010Z");
        parser.apply(json!({"type":"response_item","timestamp":"1970-01-01T00:01:07Z","payload":{"type":"function_call_output","call_id":"call"}}));
        usage(&mut parser, "three", 50, 0, "1970-01-01T00:01:08Z");
        // A child usage and inherited parent events are ignored by thread identity.
        parser.apply(json!({"type":"token_usage_record","payload":{"thread_id":"child","turn_id":"turn","response_id":"child-response","usage":{"output_tokens":9999}}}));
        complete(&mut parser);
        let (turns, _, _, _, _) = parser.finish();
        let summary = &turns[0].metrics;
        assert_eq!(summary.output_tokens, Some(452));
        assert_eq!(summary.request_count, 3);
        assert_eq!(summary.valid_request_count, 2);
        assert_eq!(summary.generation_ms, 5000);
        assert_eq!(summary.valid_output_tokens, 402);
        assert_eq!(summary.token_speed, Some(80.0));
        assert_eq!(summary.coverage, Some(402.0 / 452.0));
        assert_eq!(summary.tool_wait_ms, 60000);
        assert_eq!(summary.ttft_ms, Some(1800.0));
        assert_eq!(
            turns[0].requests[2].excluded_reason.as_deref(),
            Some("缺少输出起点")
        );
    }

    #[test]
    fn excludes_missing_reasoning_bad_segments_and_tool_wait() {
        let mut parser = Parser::new("main");
        parser.active_turn = Some("turn".into());
        output(&mut parser, "AgentMessage", "a", 1000, 2000);
        usage(
            &mut parser,
            "missing-reasoning",
            100,
            30,
            "1970-01-01T00:00:02.010Z",
        );
        output(&mut parser, "Reasoning", "zero", 3000, 3000);
        usage(&mut parser, "zero", 100, 31, "1970-01-01T00:00:03.010Z");
        output(&mut parser, "AgentMessage", "bad", 4000, 5000);
        parser.bad_line();
        usage(&mut parser, "bad", 100, 0, "1970-01-01T00:00:05.010Z");
        parser.apply(json!({"type":"response_item","timestamp":"1970-01-01T00:00:06Z","payload":{"type":"function_call","call_id":"call","name":"tool"}}));
        usage(&mut parser, "call-only", 10, 0, "1970-01-01T00:00:06.010Z");
        parser.apply(json!({"type":"response_item","timestamp":"1970-01-01T00:00:10Z","payload":{"type":"function_call_output","call_id":"call"}}));
        output(&mut parser, "AgentMessage", "overlap", 7000, 9000);
        usage(&mut parser, "overlap", 100, 0, "1970-01-01T00:00:10.010Z");
        complete(&mut parser);
        let (turns, _, _, _, complete) = parser.finish();
        assert!(!complete);
        assert_eq!(turns[0].metrics.token_speed, None);
        assert_eq!(
            turns[0]
                .requests
                .iter()
                .map(|r| r.excluded_reason.as_deref().unwrap())
                .collect::<Vec<_>>(),
            vec![
                "缺少推理计时",
                "缺少输出起点",
                "日志段不完整",
                "缺少输出起点",
                "工具等待重叠"
            ]
        );
        assert_eq!(interval_sum(vec![(10, 20), (15, 30), (40, 50)]), 30);
    }

    #[test]
    fn settled_requests_count_in_current_and_interrupted_turns() {
        for status in ["inProgress", "interrupted", "completed"] {
            let mut parser = Parser::new("main");
            parser.apply(json!({"type":"event_msg","timestamp":"1970-01-01T00:00:01.100Z","payload":{"type":"task_started","turn_id":"turn","started_at":1}}));
            output(&mut parser, "Reasoning", "reasoning", 1500, 3000);
            output(&mut parser, "AgentMessage", "text", 3000, 4000);
            usage(&mut parser, "settled", 101, 20, "1970-01-01T00:00:04.010Z");
            // The next request has output but no usage settlement yet.
            output(&mut parser, "Reasoning", "unfinished", 4500, 5000);
            if status == "interrupted" {
                parser.apply(json!({"type":"event_msg","timestamp":"1970-01-01T00:00:06Z","payload":{"type":"turn_aborted","turn_id":"turn","duration_ms":4900}}));
            } else if status == "completed" {
                complete(&mut parser);
            }
            let (turns, _, _, _, _) = parser.finish();
            assert_eq!(turns[0].metrics.request_count, 1);
            assert_eq!(turns[0].metrics.token_speed, Some(40.0));
            assert_eq!(
                turns[0].metrics.ttft_ms,
                Some(if status == "completed" {
                    1800.0
                } else {
                    1900.0
                })
            );
            assert_eq!(turns[0].metrics.ttft_estimated, status != "completed");
            assert_eq!(turns[0].status, status);
        }
    }

    #[test]
    fn first_token_estimate_avoids_tool_wait_and_native_values_take_precedence() {
        for native in [false, true] {
            let mut parser = Parser::new("main");
            parser.apply(json!({"type":"event_msg","timestamp":"1970-01-01T00:00:01Z","payload":{"type":"task_started","turn_id":"turn"}}));
            parser.apply(json!({"type":"response_item","timestamp":"1970-01-01T00:00:02Z","payload":{"type":"function_call","call_id":"tool","name":"tool"}}));
            usage(&mut parser, "tool-call", 10, 0, "1970-01-01T00:00:02.010Z");
            parser.apply(json!({"type":"response_item","timestamp":"1970-01-01T00:00:12Z","payload":{"type":"function_call_output","call_id":"tool"}}));
            output(&mut parser, "AgentMessage", "after-tool", 13000, 14000);
            usage(
                &mut parser,
                "after-tool",
                101,
                0,
                "1970-01-01T00:00:14.010Z",
            );
            if native {
                complete(&mut parser);
            }
            let (turns, _, _, _, _) = parser.finish();
            assert_eq!(turns[0].metrics.token_speed, Some(100.0));
            assert_eq!(turns[0].metrics.tool_wait_ms, 10000);
            assert_eq!(turns[0].metrics.ttft_ms, native.then_some(1800.0));
            assert!(!turns[0].metrics.ttft_estimated);
        }
    }

    #[test]
    #[ignore = "reads local metrics; requires CODEX_PANE_VERIFY_METRICS_THREAD"]
    fn local_current_and_interrupted_metrics() {
        let id = std::env::var("CODEX_PANE_VERIFY_METRICS_THREAD").unwrap();
        let result = read_at(&crate::source::codex_home().unwrap(), &id, false).unwrap();
        assert_eq!(
            result.latest_turn_id,
            result
                .turns
                .iter()
                .max_by_key(|turn| turn.started_at.unwrap_or(0))
                .map(|turn| turn.turn_id.clone())
        );
        for turn in result.turns.iter().filter(|turn| {
            turn.status == "interrupted" || Some(&turn.turn_id) == result.latest_turn_id.as_ref()
        }) {
            println!(
                "turn={} status={} metrics={}",
                turn.turn_id,
                turn.status,
                serde_json::to_string(&turn.metrics).unwrap()
            );
            if turn.turn_id == "01a163a9-fd76-7a32-a0a0-2f5c49528a28" {
                assert_eq!(turn.metrics.request_count, 32);
                assert_eq!(turn.metrics.valid_request_count, 26);
                assert_eq!(turn.metrics.valid_output_tokens, 37344);
                assert_eq!(turn.metrics.generation_ms, 1860619);
                assert_eq!(turn.metrics.ttft_ms, Some(3456.0));
                assert!(turn.metrics.ttft_estimated);
            }
        }
    }

    #[test]
    fn inherited_history_and_duplicate_usage_conflicts_do_not_create_samples() {
        let mut parser = Parser::new("child");
        parser.apply(json!({"type":"session_meta","payload":{"id":"main"}}));
        event(
            &mut parser,
            json!({"type":"task_complete","turn_id":"parent-turn","duration_ms":99999}),
        );
        parser.apply(json!({"type":"session_meta","payload":{"id":"child"}}));
        parser.active_turn = Some("own-turn".into());
        event(
            &mut parser,
            json!({"type":"task_complete","turn_id":"own-turn","duration_ms":1000}),
        );
        let (turns, _, _, _, _) = parser.finish();
        assert_eq!(turns.len(), 1);
        assert_eq!(turns[0].turn_id, "own-turn");
        let mut parser = Parser::new("main");
        parser.active_turn = Some("turn".into());
        output(&mut parser, "AgentMessage", "a", 1000, 2000);
        usage(&mut parser, "duplicate", 100, 0, "1970-01-01T00:00:02.010Z");
        usage(&mut parser, "duplicate", 200, 0, "1970-01-01T00:00:02.010Z");
        complete(&mut parser);
        let (turns, _, _, _, complete) = parser.finish();
        assert!(!complete);
        assert_eq!(turns[0].metrics.output_tokens, Some(100));
        assert_eq!(turns[0].metrics.token_speed, None);
        assert_eq!(
            turns[0].requests[0].excluded_reason.as_deref(),
            Some("用量冲突")
        );
    }

    #[test]
    #[ignore = "requires read-only access to local Codex sample logs"]
    fn local_metrics_sample() {
        let home = crate::source::codex_home().unwrap();
        for id in [
            "01a109ee-08e0-7e10-a737-a8e2d4cf3b5d",
            "01a109fc-84c3-72a2-a586-a1d939f558b0",
        ] {
            let result = read_at(&home, id, id.starts_with("01a109ee")).unwrap();
            let turn = if id.starts_with("01a109ee") {
                result
                    .turns
                    .iter()
                    .find(|t| t.turn_id == "01a109fc-3e8b-7342-9504-c5b6144be793")
                    .unwrap()
            } else {
                result
                    .turns
                    .iter()
                    .find(|t| t.metrics.request_count > 0)
                    .unwrap()
            };
            println!(
                "sample {id} {}",
                serde_json::to_string(&turn.metrics).unwrap()
            );
            let reasons: BTreeMap<_, usize> = turn
                .requests
                .iter()
                .filter_map(|r| r.excluded_reason.as_ref())
                .fold(BTreeMap::new(), |mut m, r| {
                    *m.entry(r.clone()).or_default() += 1;
                    m
                });
            println!("excluded {reasons:?}");
            assert!(turn.metrics.token_speed.is_some());
            if id.starts_with("01a109ee") {
                assert_eq!(turn.metrics.output_tokens, Some(17341));
                assert_eq!(turn.metrics.request_count, 82);
                assert_eq!(turn.metrics.valid_request_count, 61);
                assert_eq!(turn.metrics.valid_output_tokens, 14945);
                assert_eq!(turn.metrics.generation_ms, 718853);
                assert_eq!(turn.metrics.ttft_ms, Some(2663.0));
                assert!(result.subagents.iter().any(|agent| {
                    agent.thread_id == "01a109fc-84c3-72a2-a586-a1d939f558b0"
                        && agent.error.is_none()
                        && agent.summary.is_some()
                }));
            } else {
                assert_eq!(turn.metrics.output_tokens, Some(30364));
                assert_eq!(turn.metrics.request_count, 65);
                assert_eq!(turn.metrics.valid_request_count, 38);
                assert_eq!(turn.metrics.valid_output_tokens, 19794);
                assert_eq!(turn.metrics.generation_ms, 635213);
            }
        }
    }

    #[test]
    fn compressed_fork_and_subagent_usage_stay_separate() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path();
        let sessions = home.join("sessions");
        std::fs::create_dir_all(&sessions).unwrap();
        let db = Connection::open(home.join("state_5.sqlite")).unwrap();
        db.execute_batch("CREATE TABLE threads(id TEXT, rollout_path TEXT, title TEXT, cwd TEXT, model TEXT, archived INTEGER, created_at INTEGER, updated_at INTEGER);").unwrap();
        let write_log = |id: &str, records: Vec<Value>| {
            let path = sessions.join(format!("rollout-{id}.jsonl"));
            let bytes = records
                .into_iter()
                .map(|v| format!("{v}\n"))
                .collect::<String>();
            std::fs::write(&path, &bytes).unwrap();
            db.execute(
                "INSERT INTO threads VALUES(?1,?2,?1,NULL,'test',0,1,2)",
                rusqlite::params![id, path.to_str().unwrap()],
            )
            .unwrap();
            (path, bytes.len())
        };
        let request = |id: &str, tokens: u64| json!({"type":"token_usage_record","payload":{"thread_id":id,"turn_id":"turn","response_id":format!("request-{id}"),"usage":{"input_tokens":1000,"cached_input_tokens":500,"output_tokens":tokens,"reasoning_output_tokens":0,"total_tokens":1000+tokens}}});
        let start = json!({"type":"event_msg","payload":{"type":"task_started","turn_id":"turn"}});
        let end = json!({"type":"event_msg","payload":{"type":"task_complete","turn_id":"turn","time_to_first_token_ms":1234}});
        let output = json!({"type":"event_msg","payload":{"type":"item_completed","turn_id":"turn","started_at_ms":1000,"completed_at_ms":2000,"item":{"type":"AgentMessage","id":"answer"}}});
        let (parent, size) = write_log(
            "parent",
            vec![
                json!({"type":"session_meta","payload":{"id":"parent"}}),
                start.clone(),
                output.clone(),
                request("parent", 900),
                end.clone(),
            ],
        );
        let (child, _) = write_log(
            "agent",
            vec![
                json!({"type":"session_meta","payload":{"id":"agent","thread_source":"subagent","parent_thread_id":"main"}}),
                start.clone(),
                output.clone(),
                request("agent", 75),
                end.clone(),
            ],
        );
        let header = json!({"type":"session_meta","payload":{"id":"main","history_base":{"thread_id":"parent","end_byte_offset":size,"end_ordinal_exclusive":5}}});
        let (main, _) = write_log(
            "main",
            vec![
                header.clone(),
                start.clone(),
                output,
                request("agent", 5000),
                request("main", 100),
                json!({"type":"event_msg","payload":{"type":"item_completed","turn_id":"turn","item":{"type":"SubAgentActivity","id":"agent-event","agent_thread_id":"agent","agent_path":"/root/agent","kind":"completed"}}}),
                end.clone(),
            ],
        );
        let before = read_at(home, "main", true).unwrap();
        assert_eq!(before.summary.output_tokens, Some(100));
        assert_eq!(before.summary.request_count, 1);
        assert_eq!(
            serde_json::to_value(&before.consumption).unwrap()["totals"]["output"],
            100
        );
        assert!(before.subagents[0].consumption.is_none());
        assert!(
            before.subagents[0].summary.is_none(),
            "parent details do not read child statistics"
        );
        let child_data = read_at(home, "agent", false).unwrap();
        assert_eq!(child_data.summary.output_tokens, Some(75));
        assert_eq!(
            serde_json::to_value(&child_data.consumption).unwrap()["totals"]["output"],
            75
        );
        for path in [&parent, &main, &child] {
            crate::rollout_log::compress_fixture(path);
        }
        let after = read_at(home, "main", true).unwrap();
        assert_eq!(
            serde_json::to_value(&before.consumption).unwrap(),
            serde_json::to_value(&after.consumption).unwrap()
        );
        assert_eq!(
            serde_json::to_value(&after.summary).unwrap(),
            serde_json::to_value(&before.summary).unwrap()
        );
        assert!(after.subagents[0].summary.is_none());
        assert_eq!(
            read_at(home, "agent", false).unwrap().summary.output_tokens,
            Some(75)
        );
        // A legacy cumulative snapshot may include the fork base; do not use it as own usage.
        std::fs::write(&main, [header, start, json!({"type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":9000,"output_tokens":9000}}}}), end].into_iter().map(|r|format!("{r}\n")).collect::<String>()).unwrap();
        assert_eq!(
            read_at(home, "main", false).unwrap().summary.output_tokens,
            None
        );
    }

    #[test]
    #[ignore = "requires read-only access to the complete local Codex catalog"]
    fn local_compressed_catalog() {
        let home = crate::source::codex_home().unwrap();
        let db = Connection::open_with_flags(
            home.join("state_5.sqlite"),
            OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .unwrap();
        let ids = db
            .prepare("SELECT id FROM threads")
            .unwrap()
            .query_map([], |row| row.get::<_, String>(0))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        let started = std::time::Instant::now();
        let mut failures = Vec::new();
        for id in &ids {
            if let Err(error) = read_at(&home, id, false) {
                failures.push((id.clone(), error));
            }
        }
        println!(
            "catalog total={} failures={failures:?} elapsed={:?}",
            ids.len(),
            started.elapsed()
        );
        assert!(failures.is_empty());
        let snapshot = crate::source::scan().unwrap();
        println!("source snapshot chats={}", snapshot.chats.len());
    }

    #[test]
    fn activity_details_match_log_shapes_without_exposing_encrypted_content() {
        let item = json!({"type":"McpToolCall","arguments":{"files":[{"path":"C:/one.rs"},{"path":"C:/two.rs"}]}});
        assert_eq!(
            item_detail(&item).as_deref(),
            Some("path: C:/one.rs\npath: C:/two.rs")
        );
        assert_eq!(
            item_detail(&json!({"type":"AgentMessage","content":[{"type":"text","text":"done"}]}))
                .as_deref(),
            Some("done")
        );
        assert_eq!(
            item_detail(&json!({"type":"FileChange","changes":{"C:/one.rs":{"type":"update"}}}))
                .as_deref(),
            Some("C:/one.rs")
        );
        assert_eq!(
            item_detail(&json!({"type":"Reasoning","encrypted_content":"secret"})),
            None
        );
        assert_eq!(
            argument_detail(&json!({"arguments":"{\"file_path\":\"C:/one.rs\"}"})).as_deref(),
            Some("file_path: C:/one.rs")
        );
        assert_eq!(short("a".repeat(5000)).unwrap().len(), 4000);
    }
}
