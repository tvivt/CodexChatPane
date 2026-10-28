use super::{context::database, pricing};
use crate::{metrics, rollout_log, source::SourceSnapshot};
use rusqlite::{params, Connection, OpenFlags, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    path::Path,
    sync::{Mutex, OnceLock},
};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Entry {
    model: Option<String>,
    tier: Option<String>,
    input: u64,
    cached: u64,
    output: u64,
    reasoning: Option<u64>,
    writes: Option<u64>,
}

#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Query {
    pub from: Option<i64>,
    pub to: Option<i64>,
    pub group: Option<String>,
    pub project_id: Option<String>,
    pub thread_id: Option<String>,
    pub timezone_minutes: Option<i32>,
    pub undated_only: Option<bool>,
    pub thread_ids: Option<Vec<String>>,
    pub model: Option<String>,
    pub fast_only: Option<bool>,
    pub agent: Option<String>,
    pub source_thread_id: Option<String>,
    pub search: Option<String>,
    pub offset: Option<usize>,
    pub limit: Option<usize>,
    pub ranges: Option<Vec<Range>>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Range {
    pub from: i64,
    pub to: Option<i64>,
}

impl Query {
    fn contains(&self, at: Option<i64>) -> bool {
        if self.undated_only.unwrap_or(false) && at.is_some() {
            return false;
        }
        match at {
            Some(at) => {
                self.from.is_none_or(|from| at >= from)
                    && self.to.is_none_or(|to| at < to)
                    && self.ranges.as_ref().is_none_or(|ranges| {
                        ranges
                            .iter()
                            .any(|r| at >= r.from && r.to.is_none_or(|to| at < to))
                    })
            }
            None => self.from.is_none() && self.to.is_none() && self.ranges.is_none(),
        }
    }
    pub fn validate(&self) -> Result<(), String> {
        if self.from.zip(self.to).is_some_and(|(a, b)| a >= b) {
            return Err("时间范围无效".into());
        }
        if self.ranges.as_ref().is_some_and(|ranges| {
            ranges.is_empty() || ranges.iter().any(|r| r.to.is_some_and(|to| r.from >= to))
        }) {
            return Err("周期范围无效".into());
        }
        if !(-840..=840).contains(&self.timezone_minutes.unwrap_or(480)) {
            return Err("时区偏移无效".into());
        }
        if ![
            "day", "chat", "project", "model", "agent", "request", "round",
        ]
        .contains(&self.group.as_deref().unwrap_or("day"))
        {
            return Err("分组无效".into());
        }
        if self.group.as_deref() == Some("round")
            && self
                .source_thread_id
                .as_ref()
                .is_none_or(|id| id.is_empty())
        {
            return Err("轮次统计需要指定对话".into());
        }
        if self
            .agent
            .as_deref()
            .is_some_and(|v| !["main", "subagent"].contains(&v))
        {
            return Err("对话类型无效".into());
        }
        if self.limit.is_some_and(|v| v == 0 || v > 200) {
            return Err("明细页大小无效".into());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Totals {
    input: u64,
    cached: u64,
    output: u64,
    reasoning: u64,
    requests: usize,
    conversations: usize,
    subagent_requests: usize,
    cost_usd: f64,
    main_usd: f64,
    subagent_usd: f64,
    unpriced_requests: usize,
    #[serde(default)]
    subagent_unpriced_requests: usize,
    estimated_requests: usize,
    #[serde(default)]
    subagent_input: u64,
    #[serde(default)]
    subagent_output: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Consumption {
    totals: Totals,
    price_date: String,
}

// Use the already parsed own requests; timing gaps do not invalidate settled token usage.
pub fn attach_metrics(data: &mut metrics::ChatMetrics) {
    let mut total = Totals::default();
    for turn in &mut data.turns {
        let mut own = Totals::default();
        for request in &mut turn.requests {
            request.cost_usd = None;
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
            if cached > input || request.reasoning_tokens.is_some_and(|n| n > output) {
                continue;
            }
            let entry = Entry {
                model: turn.model.clone(),
                tier: turn.service_tier.clone(),
                input,
                cached,
                output,
                reasoning: request.reasoning_tokens,
                writes: request.cache_write_tokens,
            };
            request.cost_usd = pricing::estimate(
                entry.model.as_deref(),
                entry.tier.as_deref(),
                input,
                cached,
                output,
                entry.writes,
            );
            add(&mut own, &entry, false);
            add(&mut total, &entry, false);
        }
        own.conversations = usize::from(own.requests > 0);
        turn.consumption = Some(own);
    }
    total.conversations = usize::from(total.requests > 0);
    data.consumption = Some(Consumption {
        totals: total,
        price_date: pricing::PRICE_DATE.into(),
    });
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Row {
    key: String,
    label: String,
    project: String,
    models: Vec<String>,
    totals: Totals,
    thread_id: Option<String>,
    project_id: String,
    at: Option<i64>,
    service_tier: Option<String>,
    is_subagent: bool,
    source_title: String,
    #[serde(default)]
    round_number: Option<usize>,
    #[serde(default)]
    completed_at: Option<i64>,
    #[serde(default)]
    status: Option<String>,
    model_totals: Vec<ModelTotals>,
    partial: bool,
    usage_recorded: bool,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelTotals {
    model: String,
    totals: Totals,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Trend {
    at: i64,
    totals: Totals,
    model_totals: Vec<ModelTotals>,
}

#[derive(Serialize, Deserialize)]
pub struct Choice {
    key: String,
    label: String,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Report {
    rows: Vec<Row>,
    totals: Totals,
    warnings: Vec<String>,
    undated_requests: usize,
    price_date: String,
    price_url: String,
    trend: Vec<Trend>,
    projects: Vec<Choice>,
    models: Vec<String>,
    total_rows: usize,
    model_totals: Vec<ModelTotals>,
    #[serde(default)]
    indexed: bool,
}

#[derive(Clone)]
struct Thread {
    id: String,
    title: String,
    project_id: String,
    project: String,
    parent: Option<String>,
}

pub(crate) type Signature = Vec<(String, String)>;
pub(crate) fn signature(home: &Path, id: &str, path: &Path) -> Result<Signature, String> {
    if let Some(signature) = metrics::analysis::cached_signature(home, id, path) {
        return Ok(signature);
    }
    let paths = crate::preview::metrics_segments(home, id, path)?;
    paths
        .into_iter()
        .map(|(p, _)| {
            let log = rollout_log::open(&p).map_err(|e| e.to_string())?;
            Ok((
                p.to_string_lossy().into_owned(),
                format!("usage-v3:{:?}", log.stamp),
            ))
        })
        .collect()
}
pub(super) fn unchanged(saved: &str, root: &Path) -> bool {
    let Ok(stamps) = serde_json::from_str::<Signature>(saved) else {
        return false;
    };
    stamps.last().is_some_and(|(p, _)| {
        rollout_log::logical_path(Path::new(p)) == rollout_log::logical_path(root)
    }) && stamps.iter().all(|(p, stamp)| {
        rollout_log::open(Path::new(p))
            .is_ok_and(|log| format!("usage-v3:{:?}", log.stamp) == *stamp)
    })
}

pub(super) fn candidates(
    home: &Path,
    query: &Query,
) -> Result<Vec<(String, String, String, String)>, String> {
    if let Some(ranges) = &query.ranges {
        let mut result = BTreeMap::new();
        for range in ranges {
            for item in candidates(
                home,
                &Query {
                    from: Some(range.from),
                    to: range.to,
                    ranges: None,
                    ..query.clone()
                },
            )? {
                result.insert(item.0.clone(), item);
            }
        }
        return Ok(result.into_values().collect());
    }
    let catalog = Connection::open_with_flags(
        home.join("state_5.sqlite"),
        OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .map_err(|e| e.to_string())?;
    let title = crate::source::thread_title_expression(&catalog)?;
    let mut statement=catalog.prepare(&format!("SELECT id,{title},rollout_path,COALESCE(thread_source,'') FROM threads WHERE COALESCE(thread_source,'') != 'guardian_review' AND (?1 IS NULL OR updated_at>=?1) AND (?2 IS NULL OR created_at<?2) ORDER BY updated_at DESC")).map_err(|e|e.to_string())?;
    let threads = statement
        .query_map(
            params![
                query
                    .from
                    .filter(|_| query.thread_ids.is_none())
                    .map(|n| n.div_euclid(1000)),
                query
                    .to
                    .filter(|_| query.thread_ids.is_none())
                    .map(|n| n.saturating_add(999).div_euclid(1000))
            ],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                ))
            },
        )
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    Ok(threads)
}

pub fn index(
    home: &Path,
    snapshot: Option<&SourceSnapshot>,
    query: &Query,
    progress: impl Fn(usize, usize),
) -> Result<(), String> {
    let identity = super::context::identity(home)?;
    let key = serde_json::to_string(&(
        "usage-v3",
        &identity,
        query.from,
        query.to,
        &query.ranges,
        &query.thread_ids,
    ))
    .map_err(|e| e.to_string())?;
    super::context::share(key, || {
        index_shared(home, snapshot, query, progress, &identity)
    })
}

fn index_shared(
    home: &Path,
    snapshot: Option<&SourceSnapshot>,
    query: &Query,
    progress: impl Fn(usize, usize),
    identity: &str,
) -> Result<(), String> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    let _guard = LOCK
        .get_or_init(Default::default)
        .lock()
        .map_err(|_| "消耗索引锁已损坏")?;
    if super::context::identity(home)? != identity {
        return Err("账号或日志来源已切换，停止旧索引".into());
    }
    let threads: Vec<_> = candidates(home, query)?
        .into_iter()
        .filter(|(id, _, path, _)| {
            query.thread_ids.as_ref().is_none_or(|ids| {
                ids.contains(id)
                    || crate::rollout_log::fragment_id(Path::new(path))
                        .is_some_and(|fragment| ids.iter().any(|id| id == fragment))
            })
        })
        .collect();
    let mut db = database(home)?;
    let projects: HashMap<_, _> = snapshot
        .into_iter()
        .flat_map(|s| s.projects.iter())
        .map(|p| (p.id.as_str(), p.name.as_str()))
        .collect();
    let chats: HashMap<_, _> = snapshot
        .into_iter()
        .flat_map(|s| s.chats.iter())
        .map(|c| (c.id.as_str(), c))
        .collect();
    db.execute_batch("BEGIN").map_err(|e| e.to_string())?;
    for chat in snapshot.into_iter().flat_map(|s| s.chats.iter()) {
        let name = projects
            .get(chat.project_id.as_str())
            .copied()
            .unwrap_or("无项目对话");
        db.execute("INSERT INTO usage_threads(id,signature,title,project_id,project) VALUES(?1,'',?2,?3,?4) ON CONFLICT(id) DO UPDATE SET title=excluded.title,project_id=excluded.project_id,project=excluded.project WHERE title IS NOT excluded.title OR project_id IS NOT excluded.project_id OR project IS NOT excluded.project",params![chat.id,chat.title,chat.project_id,name]).map_err(|e|e.to_string())?;
    }
    db.execute_batch("COMMIT").map_err(|e| e.to_string())?;
    let mut complete = true;
    for (index, (id, title, path, kind)) in threads.iter().enumerate() {
        if super::context::identity(home)? != identity {
            return Err("账号或日志来源已切换，停止旧索引".into());
        }
        progress(index, threads.len());
        let (project_id, project) = chats
            .get(id.as_str())
            .map(|c| {
                (
                    c.project_id.as_str(),
                    projects
                        .get(c.project_id.as_str())
                        .copied()
                        .unwrap_or("无项目对话"),
                )
            })
            .unwrap_or(("", "无项目对话"));
        let saved: Option<String> = db
            .query_row(
                "SELECT signature FROM usage_threads WHERE id=?1",
                [id],
                |r| r.get(0),
            )
            .optional()
            .map_err(|e| e.to_string())?;
        if saved
            .as_deref()
            .is_some_and(|s| unchanged(s, Path::new(path)))
        {
            db.execute(
                "UPDATE usage_threads SET title=?1,project_id=?2,project=?3 WHERE id=?4",
                params![title, project_id, project, id],
            )
            .map_err(|e| e.to_string())?;
            continue;
        }
        let result = (|| {
            let stamp = signature(home, id, Path::new(path))?;
            let data = metrics::read_own(home, id)?;
            let stable = data.usage_complete && stamp == signature(home, id, Path::new(path))?;
            complete &= stable;
            if super::context::identity(home)? != identity {
                return Err("账号或日志来源已切换，停止旧索引".into());
            }
            let mut issues = data.warnings.clone();
            if !data.usage_complete {
                issues.push("日志存在未完整或损坏记录，保留已有缓存".into());
            } else if !stable {
                issues.push("读取期间日志有更新，下次刷新重试".into());
            }
            if data.turns.iter().all(|t| t.requests.is_empty()) {
                issues.push("缺少逐请求用量，未分摊累计快照".into());
            }
            let tx = db.transaction().map_err(|e| e.to_string())?;
            tx.execute_batch("CREATE TEMP TABLE IF NOT EXISTS seen_requests(id TEXT PRIMARY KEY); DELETE FROM seen_requests;")
                .map_err(|e| e.to_string())?;
            for turn in &data.turns {
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
                    if cached > input || request.reasoning_tokens.is_some_and(|r| r > output) {
                        continue;
                    }
                    let entry = Entry {
                        model: turn.model.clone(),
                        tier: turn.service_tier.clone(),
                        input,
                        cached,
                        output,
                        reasoning: request.reasoning_tokens,
                        writes: request.cache_write_tokens,
                    };
                    save_request(&tx, id, request, &entry)?;
                    tx.execute(
                        "INSERT OR IGNORE INTO seen_requests VALUES(?1)",
                        [&request.response_id],
                    )
                    .map_err(|e| e.to_string())?;
                }
            }
            if stable {
                tx.execute("DELETE FROM usage_requests WHERE thread_id=?1 AND response_id NOT IN (SELECT id FROM seen_requests)", [id]).map_err(|e|e.to_string())?;
            }
            let parent = data
                .parent_thread_id
                .or_else(|| (kind == "subagent").then(String::new));
            tx.execute("INSERT INTO usage_threads(id,signature,title,project_id,project,parent,error) VALUES(?1,?2,?3,?4,?5,?6,?7) ON CONFLICT(id) DO UPDATE SET signature=excluded.signature,title=excluded.title,project_id=excluded.project_id,project=excluded.project,parent=excluded.parent,error=excluded.error",params![id,if stable {serde_json::to_string(&stamp).map_err(|e|e.to_string())?} else {String::new()},title,project_id,project,parent,(!issues.is_empty()).then(||issues.join("；"))]).map_err(|e|e.to_string())?;
            tx.commit().map_err(|e| e.to_string())
        })();
        if super::context::identity(home)? != identity {
            return Err("账号或日志来源已切换，停止旧索引".into());
        }
        if let Err(error) = result {
            complete = false;
            db.execute("INSERT INTO usage_threads(id,signature,title,project_id,project,error) VALUES(?1,'',?2,?3,?4,?5) ON CONFLICT(id) DO UPDATE SET signature='',error=excluded.error",params![id,title,project_id,project,error]).map_err(|e|e.to_string())?;
        }
    }
    if complete && query.thread_ids.is_none() {
        for range in query.ranges.clone().unwrap_or_else(|| {
            vec![Range {
                from: query.from.unwrap_or(i64::MIN),
                to: query.to,
            }]
        }) {
            let from = if query.ranges.is_none() {
                query.from
            } else {
                Some(range.from)
            };
            db.execute(
                "INSERT OR REPLACE INTO usage_indexed_ranges VALUES(?1,?2,?3)",
                params![
                    serde_json::to_string(&(from, range.to)).map_err(|e| e.to_string())?,
                    from,
                    range.to
                ],
            )
            .map_err(|e| e.to_string())?;
        }
    }
    // Keep previously indexed usage outside this range for later lazy-loaded periods.
    progress(threads.len(), threads.len());
    Ok(())
}

fn add(total: &mut Totals, entry: &Entry, child: bool) {
    total.input += entry.input;
    total.cached += entry.cached;
    total.output += entry.output;
    total.reasoning += entry.reasoning.unwrap_or(0);
    total.requests += 1;
    if child {
        total.subagent_requests += 1;
        total.subagent_input += entry.input;
        total.subagent_output += entry.output;
    }
    if entry.tier.is_none() || entry.writes.is_none() || entry.writes.unwrap_or(0) > 0 {
        total.estimated_requests += 1;
    }
    match pricing::estimate(
        entry.model.as_deref(),
        entry.tier.as_deref(),
        entry.input,
        entry.cached,
        entry.output,
        entry.writes,
    ) {
        Some(cost) => {
            total.cost_usd += cost;
            if child {
                total.subagent_usd += cost;
            } else {
                total.main_usd += cost;
            }
        }
        None => {
            total.unpriced_requests += 1;
            if child {
                total.subagent_unpriced_requests += 1;
            }
        }
    }
}

pub(crate) mod rounds;

fn sync_titles(home: &Path, db: &Connection) -> Result<(), String> {
    let path = home.join("state_5.sqlite");
    if !path.is_file() {
        return Ok(());
    }
    let catalog = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|e| e.to_string())?;
    let title = crate::source::thread_title_expression(&catalog)?;
    let mut statement = catalog
        .prepare(&format!("SELECT id,{title} FROM threads"))
        .map_err(|e| e.to_string())?;
    for row in statement
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(|e| e.to_string())?
    {
        let (id, title) = row.map_err(|e| e.to_string())?;
        db.execute(
            "UPDATE usage_threads SET title=?2 WHERE id=?1 AND title IS NOT ?2",
            params![id, title],
        )
        .map_err(|e| e.to_string())?;
    }
    Ok(())
}

pub fn report(home: &Path, query: Query) -> Result<Report, String> {
    query.validate()?;
    let offset = query.timezone_minutes.unwrap_or(480);
    let group = query.group.as_deref().unwrap_or("day");
    let round_details = if group == "round" {
        Some(rounds::read(
            home,
            query.source_thread_id.as_deref().unwrap(),
        )?)
    } else {
        None
    };
    let db = database(home)?;
    db.execute_batch("BEGIN").map_err(|e| e.to_string())?;
    // Refresh labels before checking cached reports; token rows do not need reparsing.
    sync_titles(home, &db)?;
    let key = format!(
        "details-v7|{}:{}|{}",
        pricing::PRICE_DATE,
        pricing::POLICY_VERSION,
        serde_json::to_string(&query).map_err(|e| e.to_string())?
    );
    let metadata: i64 = db
        .query_row("SELECT revision FROM usage_metadata WHERE id=1", [], |r| {
            r.get(0)
        })
        .map_err(|e| e.to_string())?;
    let days = db.prepare("SELECT day,revision FROM usage_days WHERE day=-1 OR ((?1 IS NULL OR day>=?1) AND (?2 IS NULL OR day<=?2)) ORDER BY day")
        .map_err(|e|e.to_string())?.query_map(params![query.from.map(|v|v.div_euclid(86400000)),query.to.map(|v|v.saturating_sub(1).div_euclid(86400000))], |r|Ok((r.get::<_,i64>(0)?,r.get::<_,i64>(1)?)))
        .map_err(|e|e.to_string())?.collect::<Result<Vec<_>,_>>().map_err(|e|e.to_string())?;
    let stamp = format!(
        "{metadata}|{days:?}|{}",
        round_details
            .as_ref()
            .map_or("", |(_, stamp)| stamp.as_str())
    );
    let cached: Option<String> = db
        .query_row(
            "SELECT data FROM usage_reports WHERE key=?1 AND stamp=?2",
            params![key, stamp],
            |r| r.get(0),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    if let Some(mut report) = cached.and_then(|s| serde_json::from_str::<Report>(&s).ok()) {
        report.indexed = covered(&db, &query)?;
        return Ok(report);
    }
    let mut statement = db
        .prepare("SELECT id,title,project_id,project,parent,error FROM usage_threads")
        .map_err(|e| e.to_string())?;
    let mut threads = HashMap::new();
    let mut warnings = vec![];
    let relevant =
        if (query.from.is_some() || query.to.is_some()) && home.join("state_5.sqlite").is_file() {
            Some(
                candidates(home, &query)?
                    .into_iter()
                    .map(|(id, _, _, _)| id)
                    .collect::<HashSet<_>>(),
            )
        } else {
            None
        };
    for row in statement
        .query_map([], |r| {
            Ok((
                Thread {
                    id: r.get(0)?,
                    title: r.get(1)?,
                    project_id: r.get(2)?,
                    project: r.get(3)?,
                    parent: r.get(4)?,
                },
                r.get::<_, Option<String>>(5)?,
            ))
        })
        .map_err(|e| e.to_string())?
    {
        let (thread, error) = row.map_err(|e| e.to_string())?;
        if let Some(error) = error {
            if relevant.as_ref().is_none_or(|ids| ids.contains(&thread.id)) {
                warnings.push(format!("{}：{}", thread.title, error));
            }
        }
        threads.insert(thread.id.clone(), thread);
    }
    let mut totals = Totals::default();
    let mut rows: BTreeMap<String, Row> = BTreeMap::new();
    let mut conversations = HashSet::new();
    let mut per_group: HashMap<String, HashSet<String>> = HashMap::new();
    let mut trend: BTreeMap<i64, Totals> = BTreeMap::new();
    let mut trend_conversations: HashMap<i64, HashSet<String>> = HashMap::new();
    let mut model_totals: BTreeMap<String, Totals> = BTreeMap::new();
    let mut row_models: HashMap<String, BTreeMap<String, Totals>> = HashMap::new();
    let mut trend_models: HashMap<i64, BTreeMap<String, Totals>> = HashMap::new();
    let mut model_chats: HashMap<String, HashSet<String>> = HashMap::new();
    let mut row_model_chats: HashMap<(String, String), HashSet<String>> = HashMap::new();
    let mut trend_model_chats: HashMap<(i64, String), HashSet<String>> = HashMap::new();
    let mut projects = BTreeMap::new();
    let mut models = std::collections::BTreeSet::new();
    let search = query.search.as_deref().unwrap_or("").trim().to_lowercase();
    let bucket = 86400000;
    let shift = i64::from(offset) * 60000;
    let undated = db
        .query_row(
            "SELECT count(*) FROM usage_requests WHERE at IS NULL",
            [],
            |r| r.get::<_, usize>(0),
        )
        .map_err(|e| e.to_string())?;
    let mut statement=db.prepare("SELECT thread_id,at,data,response_id FROM usage_requests WHERE (?1 IS NULL OR at>=?1) AND (?2 IS NULL OR at<?2) AND (?3=0 OR at IS NULL)").map_err(|e|e.to_string())?;
    for item in statement
        .query_map(
            params![query.from, query.to, query.undated_only.unwrap_or(false)],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, Option<i64>>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                ))
            },
        )
        .map_err(|e| e.to_string())?
    {
        let (id, at, data, response_id) = item.map_err(|e| e.to_string())?;
        if !query.contains(at) {
            continue;
        }
        let Some(thread) = threads.get(&id) else {
            continue;
        };
        let mut root = thread;
        let mut visited = HashSet::new();
        while let Some(parent) = root.parent.as_ref().and_then(|p| threads.get(p)) {
            if !visited.insert(root.id.as_str()) {
                break;
            }
            root = parent;
        }
        let entry: Entry = serde_json::from_str(&data).map_err(|e| e.to_string())?;
        let model = entry.model.as_deref().unwrap_or("未知");
        let child = thread.parent.is_some();
        projects.insert(root.project_id.clone(), root.project.clone());
        models.insert(model.to_string());
        if query
            .project_id
            .as_ref()
            .is_some_and(|p| p != &root.project_id)
            || query.thread_id.as_ref().is_some_and(|t| t != &root.id)
            || query.source_thread_id.as_ref().is_some_and(|t| t != &id)
            || query.model.as_ref().is_some_and(|m| m != model)
            || (query.fast_only.unwrap_or(false)
                && !entry
                    .tier
                    .as_deref()
                    .is_some_and(|v| matches!(v, "fast" | "priority")))
            || query
                .agent
                .as_deref()
                .is_some_and(|v| (v == "subagent") != child)
            || (!search.is_empty()
                && !format!("{} {} {} {}", root.title, thread.title, root.project, model)
                    .to_lowercase()
                    .contains(&search))
        {
            continue;
        }
        let round = round_details.as_ref().and_then(|(details, _)| {
            details
                .requests
                .get(&response_id)
                .and_then(|index| details.rounds.get(*index))
                .filter(|round| !round.prompt.is_empty())
        });
        let key = match group {
            "chat" => root.id.clone(),
            "round" => format!(
                "{id}/{}",
                round.map_or("unknown".into(), |r| r.number.to_string())
            ),
            "project" => root.project_id.clone(),
            "model" => model.to_string(),
            "agent" => id.clone(),
            "request" => format!("{id}/{response_id}"),
            _ => at
                .and_then(|at| {
                    time::OffsetDateTime::from_unix_timestamp(
                        at.div_euclid(1000) + i64::from(offset) * 60,
                    )
                    .ok()
                })
                .map(|t| t.date().to_string())
                .unwrap_or_else(|| "未分配".into()),
        };
        let label = match group {
            "chat" => root.title.clone(),
            "round" => round.map_or_else(|| "轮次未识别".into(), |r| r.prompt.clone()),
            "project" => root.project.clone(),
            "model" => model.to_string(),
            "agent" => thread.title.clone(),
            "request" => response_id.clone(),
            _ => key.clone(),
        };
        let row = rows.entry(key.clone()).or_insert_with(|| Row {
            key: key.clone(),
            label,
            project: root.project.clone(),
            models: vec![],
            totals: Totals::default(),
            thread_id: matches!(group, "chat" | "agent" | "request" | "round").then(|| {
                if group == "chat" {
                    root.id.clone()
                } else {
                    id.clone()
                }
            }),
            project_id: root.project_id.clone(),
            at: if group == "round" {
                round.and_then(|r| r.started_at)
            } else {
                at
            },
            service_tier: matches!(group, "request")
                .then(|| entry.tier.clone())
                .flatten(),
            is_subagent: if group == "chat" {
                root.parent.is_some()
            } else {
                matches!(group, "agent" | "request" | "round") && child
            },
            source_title: thread.title.clone(),
            round_number: round.map(|r| r.number),
            completed_at: round.and_then(|r| r.completed_at),
            status: round.map(|r| r.status.clone()),
            model_totals: vec![],
            partial: round.is_some_and(|r| {
                !query.contains(r.started_at)
                    || r.completed_at
                        .is_some_and(|at| !query.contains(Some(at.saturating_sub(1))))
                    || r.request_times.iter().any(|at| !query.contains(*at))
            }),
            usage_recorded: true,
        });
        if let Some(model) = &entry.model {
            if !row.models.contains(model) {
                row.models.push(model.clone());
            }
        }
        add(&mut row.totals, &entry, child);
        add(&mut totals, &entry, child);
        let model_total = model_totals.entry(model.into()).or_default();
        add(model_total, &entry, child);
        model_total.conversations = {
            let ids = model_chats.entry(model.into()).or_default();
            ids.insert(root.id.clone());
            ids.len()
        };
        let row_total = row_models
            .entry(key.clone())
            .or_default()
            .entry(model.into())
            .or_default();
        add(row_total, &entry, child);
        row_total.conversations = {
            let ids = row_model_chats
                .entry((key.clone(), model.into()))
                .or_default();
            ids.insert(root.id.clone());
            ids.len()
        };
        if let Some(at) = at {
            let at = (at + shift).div_euclid(bucket) * bucket - shift;
            add(trend.entry(at).or_default(), &entry, child);
            let model_total = trend_models
                .entry(at)
                .or_default()
                .entry(model.into())
                .or_default();
            add(model_total, &entry, child);
            model_total.conversations = {
                let ids = trend_model_chats.entry((at, model.into())).or_default();
                ids.insert(root.id.clone());
                ids.len()
            };
            trend_conversations
                .entry(at)
                .or_default()
                .insert(root.id.clone());
        }
        conversations.insert(root.id.clone());
        per_group.entry(key).or_default().insert(root.id.clone());
    }
    if let Some((details, _)) = &round_details {
        let id = query.source_thread_id.as_deref().unwrap();
        if let Some(thread) = threads.get(id).filter(|thread| {
            query
                .project_id
                .as_ref()
                .is_none_or(|project| project == &thread.project_id)
                && query.thread_id.as_ref().is_none_or(|root| root == id)
                && query
                    .agent
                    .as_deref()
                    .is_none_or(|role| (role == "subagent") == thread.parent.is_some())
                && (search.is_empty()
                    || format!("{} {}", thread.title, thread.project)
                        .to_lowercase()
                        .contains(&search))
        }) {
            for round in details.rounds.iter().filter(|r| {
                !r.prompt.is_empty() && r.started_at.is_some_and(|at| query.contains(Some(at)))
            }) {
                let key = format!("{id}/{}", round.number);
                rows.entry(key.clone()).or_insert_with(|| Row {
                    key,
                    label: round.prompt.clone(),
                    project: thread.project.clone(),
                    models: vec![],
                    totals: Totals::default(),
                    thread_id: Some(id.into()),
                    project_id: thread.project_id.clone(),
                    at: round.started_at,
                    service_tier: None,
                    is_subagent: thread.parent.is_some(),
                    source_title: thread.title.clone(),
                    round_number: Some(round.number),
                    completed_at: round.completed_at,
                    status: Some(round.status.clone()),
                    model_totals: vec![],
                    partial: round.request_times.iter().any(|at| !query.contains(*at))
                        || round
                            .completed_at
                            .is_some_and(|at| !query.contains(Some(at.saturating_sub(1)))),
                    usage_recorded: round.request_times.iter().any(|at| query.contains(*at)),
                });
            }
        }
    }
    totals.conversations = conversations.len();
    let mut rows: Vec<_> = rows.into_values().collect();
    for row in &mut rows {
        row.totals.conversations = per_group.get(&row.key).map_or(0, HashSet::len);
        row.model_totals = model_rows(row_models.remove(&row.key).unwrap_or_default());
    }
    if group == "round" {
        rows.sort_by(|a, b| b.round_number.cmp(&a.round_number));
    } else if group == "request" {
        rows.sort_by(|a, b| b.at.cmp(&a.at).then_with(|| a.key.cmp(&b.key)));
    } else if group == "day" {
        rows.sort_by(|a, b| b.key.cmp(&a.key));
    } else {
        rows.sort_by(|a, b| b.totals.cost_usd.total_cmp(&a.totals.cost_usd));
    }
    let total_rows = rows.len();
    if group == "request" {
        rows = rows
            .into_iter()
            .skip(query.offset.unwrap_or(0))
            .take(query.limit.unwrap_or(50))
            .collect();
    }
    let report = Report {
        rows,
        totals,
        warnings,
        undated_requests: undated,
        price_date: pricing::PRICE_DATE.into(),
        price_url: pricing::PRICE_URL.into(),
        total_rows,
        model_totals: model_rows(model_totals),
        indexed: covered(&db, &query)?,
        trend: trend
            .into_iter()
            .map(|(at, mut totals)| {
                totals.conversations = trend_conversations.get(&at).map_or(0, HashSet::len);
                Trend {
                    at,
                    totals,
                    model_totals: model_rows(trend_models.remove(&at).unwrap_or_default()),
                }
            })
            .collect(),
        projects: projects
            .into_iter()
            .map(|(key, label)| Choice { key, label })
            .collect(),
        models: models.into_iter().collect(),
    };
    db.execute_batch("COMMIT").map_err(|e| e.to_string())?;
    db.execute(
        "INSERT OR REPLACE INTO usage_reports VALUES(?1,?2,?3)",
        params![
            key,
            stamp,
            serde_json::to_string(&report).map_err(|e| e.to_string())?
        ],
    )
    .map_err(|e| e.to_string())?;
    Ok(report)
}

fn model_rows(models: BTreeMap<String, Totals>) -> Vec<ModelTotals> {
    models
        .into_iter()
        .map(|(model, totals)| ModelTotals { model, totals })
        .collect()
}

fn covered(db: &Connection, query: &Query) -> Result<bool, String> {
    let mut statement = db
        .prepare("SELECT from_at,to_at FROM usage_indexed_ranges")
        .map_err(|e| e.to_string())?;
    let mut saved = statement
        .query_map([], |r| {
            Ok((r.get::<_, Option<i64>>(0)?, r.get::<_, Option<i64>>(1)?))
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    let ranges = query
        .ranges
        .as_ref()
        .map(|rs| rs.iter().map(|r| (Some(r.from), r.to)).collect::<Vec<_>>())
        .unwrap_or_else(|| vec![(query.from, query.to)]);
    saved.sort_by_key(|(from, _)| from.unwrap_or(i64::MIN));
    Ok(ranges.iter().all(|(from, to)| {
        let mut cursor = from.unwrap_or(i64::MIN);
        let end = to.unwrap_or(i64::MAX);
        for (old_from, old_to) in &saved {
            if old_from.unwrap_or(i64::MIN) > cursor {
                break;
            }
            cursor = cursor.max(old_to.unwrap_or(i64::MAX));
            if cursor >= end {
                return true;
            }
        }
        false
    }))
}

fn save_request(
    db: &Connection,
    id: &str,
    request: &metrics::Request,
    entry: &Entry,
) -> Result<(), String> {
    db.execute("INSERT INTO usage_requests(thread_id,response_id,at,data) VALUES(?1,?2,?3,?4) ON CONFLICT(thread_id,response_id) DO UPDATE SET at=excluded.at,data=excluded.data WHERE usage_requests.at IS NOT excluded.at OR usage_requests.data IS NOT excluded.data",
        params![id,request.response_id,request.settled_at,serde_json::to_string(entry).map_err(|e|e.to_string())?]).map_err(|e|e.to_string())?;
    Ok(())
}

// Partial metrics reads can add settled requests, but cannot certify a whole log as indexed.
pub fn reuse(home: &Path, data: &metrics::ChatMetrics) -> Result<(), String> {
    let mut db = database(home)?;
    let tx = db.transaction().map_err(|e| e.to_string())?;
    tx.execute("INSERT OR IGNORE INTO usage_threads(id,signature,title,project_id,project,parent) VALUES(?1,'',?2,'','无项目对话',?3)", params![data.thread_id,data.title,data.parent_thread_id]).map_err(|e|e.to_string())?;
    for (model, tier, requests) in &data.usage_batches {
        for request in requests {
            if matches!(
                request.excluded_reason.as_deref(),
                Some("用量冲突" | "用量字段无效")
            ) {
                tx.execute(
                    "DELETE FROM usage_requests WHERE thread_id=?1 AND response_id=?2",
                    params![data.thread_id, request.response_id],
                )
                .map_err(|e| e.to_string())?;
                continue;
            }
            let (Some(input), Some(cached), Some(output)) = (
                request.input_tokens,
                request.cached_input_tokens,
                request.output_tokens,
            ) else {
                continue;
            };
            if cached > input || request.reasoning_tokens.is_some_and(|v| v > output) {
                continue;
            }
            save_request(
                &tx,
                &data.thread_id,
                request,
                &Entry {
                    model: model.clone(),
                    tier: tier.clone(),
                    input,
                    cached,
                    output,
                    reasoning: request.reasoning_tokens,
                    writes: request.cache_write_tokens,
                },
            )?;
        }
    }
    tx.commit().map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    #[test]
    fn zero_rated_requests_keep_tokens_roles_and_invalidate_old_price_reports() {
        let dir = tempfile::tempdir().unwrap();
        super::super::context::confirm(dir.path(), Some("test")).unwrap();
        let db = database(dir.path()).unwrap();
        db.execute_batch("INSERT INTO usage_threads VALUES('main','','Main','','Project',NULL,NULL); INSERT INTO usage_threads VALUES('child','','Child','','Project','main',NULL);").unwrap();
        for (i, model) in [
            None,
            Some("gpt-reserve"),
            Some("codex-auto-review"),
            Some("gpt-5.3-codex-spark"),
            Some("gpt-6.1-sol"),
        ]
        .into_iter()
        .enumerate()
        {
            let entry = Entry {
                model: model.map(str::to_owned),
                tier: Some("default".into()),
                input: 100,
                cached: 90,
                output: 10,
                reasoning: Some(0),
                writes: Some(0),
            };
            db.execute(
                "INSERT INTO usage_requests VALUES(?1,?2,?3,?4)",
                params![
                    if i % 2 == 0 { "main" } else { "child" },
                    format!("request-{i}"),
                    1000 + i as i64,
                    serde_json::to_string(&entry).unwrap()
                ],
            )
            .unwrap();
        }
        let query = Query::default();
        let first = report(dir.path(), query.clone()).unwrap();
        let mut old = serde_json::to_value(&first).unwrap();
        old["totals"]["unpricedRequests"] = serde_json::json!(4);
        let stamp: String = db
            .query_row("SELECT stamp FROM usage_reports LIMIT 1", [], |r| r.get(0))
            .unwrap();
        db.execute("DELETE FROM usage_reports", []).unwrap();
        let old_key = format!(
            "details-v7|{}|{}",
            pricing::PRICE_DATE,
            serde_json::to_string(&query).unwrap()
        );
        db.execute(
            "INSERT INTO usage_reports VALUES(?1,?2,?3)",
            params![old_key, stamp, old.to_string()],
        )
        .unwrap();
        let priced = serde_json::to_value(report(dir.path(), query).unwrap()).unwrap();
        assert_eq!(priced["totals"]["unpricedRequests"], 0);
        assert_eq!(priced["totals"]["requests"], 5);
        assert_eq!(priced["totals"]["input"], 500);
        assert_eq!(priced["totals"]["output"], 50);
        assert_eq!(priced["totals"]["subagentRequests"], 2);
        assert!(priced["totals"]["costUsd"].as_f64().unwrap() > 0.0);
        assert_eq!(priced["totals"]["subagentUsd"], 0.0);
        assert_eq!(
            db.query_row("SELECT count(*) FROM usage_requests", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            5
        );
        assert_eq!(
            db.query_row("SELECT count(*) FROM usage_reports", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            2
        );
    }
    #[test]
    fn adjacent_indexed_ranges_cover_a_merged_period_without_a_new_scan() {
        let dir = tempfile::tempdir().unwrap();
        super::super::context::confirm(dir.path(), Some("test")).unwrap();
        let db = super::database(dir.path()).unwrap();
        db.execute(
            "INSERT INTO usage_indexed_ranges(from_at,to_at) VALUES(10,20),(20,30)",
            [],
        )
        .unwrap();
        let query = super::Query {
            from: Some(10),
            to: Some(30),
            ..Default::default()
        };
        assert!(super::covered(&db, &query).unwrap());
        db.execute(
            "UPDATE usage_indexed_ranges SET to_at=19 WHERE from_at=10",
            [],
        )
        .unwrap();
        assert!(!super::covered(&db, &query).unwrap());
    }
    use super::*;

    #[test]
    fn legacy_rounds_keep_empty_questions_partial_usage_and_unknown_requests_reconciled() {
        use serde_json::json;
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path();
        super::super::context::confirm(home, Some("test-account")).unwrap();
        let catalog = Connection::open(home.join("state_5.sqlite")).unwrap();
        catalog.execute_batch("CREATE TABLE threads(id TEXT,title TEXT,rollout_path TEXT,created_at INTEGER,updated_at INTEGER,thread_source TEXT,cwd TEXT,model TEXT,archived INTEGER DEFAULT 0);").unwrap();
        let main = home.join("main.jsonl");
        let child = home.join("child.jsonl");
        for (id, path, kind) in [("main", &main, "user"), ("child", &child, "subagent")] {
            catalog.execute("INSERT INTO threads(id,title,rollout_path,created_at,updated_at,thread_source) VALUES(?1,?1,?2,0,10,?3)",params![id,path.to_string_lossy(),kind]).unwrap();
        }
        let old = |at: &str, total: u64, last: u64| {
            json!({"type":"event_msg","timestamp":at,"payload":{"type":"token_count","info":{
            "total_token_usage":{"input_tokens":total,"cached_input_tokens":total/2,"output_tokens":total/10},
            "last_token_usage":{"input_tokens":last,"cached_input_tokens":last/2,"output_tokens":last/10}}}})
        };
        let records = vec![
            json!({"type":"session_meta","payload":{"id":"main"}}),
            json!({"timestamp":"1970-01-01T00:00:00Z","type":"event_msg","payload":{"type":"task_started","turn_id":"one"}}),
            json!({"type":"turn_context","payload":{"turn_id":"one","model":"gpt-6.1-sol"}}),
            json!({"timestamp":"1970-01-01T00:00:00Z","type":"event_msg","payload":{"type":"user_message","message":"First question"}}),
            old("1970-01-01T00:00:01Z", 100, 100),
            old("1970-01-01T00:00:02Z", 300, 200),
            json!({"timestamp":"1970-01-01T00:00:02.100Z","type":"token_usage_record","payload":{"turn_id":"unseen","response_id":"unmapped","usage":{"input_tokens":50,"cached_input_tokens":0,"output_tokens":5}}}),
            json!({"type":"event_msg","payload":{"type":"turn_aborted","turn_id":"one"}}),
            json!({"type":"event_msg","payload":{"type":"task_started","turn_id":"resume"}}),
            json!({"type":"turn_context","payload":{"turn_id":"resume","model":"gpt-6.1-sol"}}),
            json!({"type":"event_msg","payload":{"type":"user_message","message":"继续"}}),
            json!({"timestamp":"1970-01-01T00:00:03Z","type":"event_msg","payload":{"type":"task_complete","turn_id":"resume"}}),
            json!({"type":"event_msg","payload":{"type":"task_started","turn_id":"two"}}),
            json!({"timestamp":"1970-01-01T00:00:03.500Z","type":"event_msg","payload":{"type":"user_message","message":"Question without recorded usage"}}),
            json!({"timestamp":"1970-01-01T00:00:04Z","type":"event_msg","payload":{"type":"task_complete","turn_id":"two"}}),
        ];
        let text = records.iter().map(|v| format!("{v}\n")).collect::<String>();
        std::fs::write(
            main.with_extension("jsonl.zst"),
            zstd::stream::encode_all(text.as_bytes(), 1).unwrap(),
        )
        .unwrap();
        let records = [
            json!({"type":"session_meta","payload":{"id":"child","thread_source":"subagent","parent_thread_id":"main"}}),
            json!({"type":"turn_context","payload":{"turn_id":"worker","model":"gpt-6-astra"}}),
            json!({"timestamp":"1970-01-01T00:00:02.200Z","type":"token_usage_record","payload":{"turn_id":"worker","response_id":"child-request","usage":{"input_tokens":900,"cached_input_tokens":450,"output_tokens":90}}}),
        ];
        std::fs::write(
            child,
            records.iter().map(|v| format!("{v}\n")).collect::<String>(),
        )
        .unwrap();
        let query = Query {
            from: Some(1500),
            to: Some(4500),
            timezone_minutes: Some(0),
            group: Some("chat".into()),
            ..Default::default()
        };
        index(home, None, &query, |_, _| {}).unwrap();
        let family = report(home, query.clone()).unwrap();
        assert_eq!(family.totals.input, 1150);
        assert_eq!(family.totals.cached, 550);
        assert_eq!(
            family
                .model_totals
                .iter()
                .map(|m| m.totals.input)
                .sum::<u64>(),
            1150
        );
        assert_eq!(
            family.rows[0]
                .model_totals
                .iter()
                .map(|m| m.totals.input)
                .sum::<u64>(),
            1150
        );
        assert_eq!(
            family.trend.len(),
            1,
            "all requests belong to one local natural day"
        );
        assert_eq!(
            family.trend[0]
                .model_totals
                .iter()
                .map(|m| m.totals.input)
                .sum::<u64>(),
            1150
        );
        assert_eq!(
            family.totals.unpriced_requests, 0,
            "missing-model usage remains counted and follows the explicit zero-price policy"
        );
        let own = report(
            home,
            Query {
                group: Some("round".into()),
                thread_id: Some("main".into()),
                source_thread_id: Some("main".into()),
                agent: Some("main".into()),
                ..query.clone()
            },
        )
        .unwrap();
        assert_eq!(own.totals.input, 250);
        assert_eq!(
            own.rows.iter().map(|r| r.totals.input).sum::<u64>(),
            own.totals.input
        );
        let first = own
            .rows
            .iter()
            .find(|r| r.label == "First question")
            .unwrap();
        assert_eq!(
            first.totals.input, 200,
            "legacy request inherits the same technical turn as conversation details"
        );
        assert!(first.partial);
        let empty = own
            .rows
            .iter()
            .find(|r| r.label == "Question without recorded usage")
            .unwrap();
        assert!(!empty.usage_recorded);
        let unknown = own.rows.iter().find(|r| r.round_number.is_none()).unwrap();
        assert_eq!(
            unknown.totals.input, 50,
            "unmapped requests stay in the reconciliation total"
        );
        let agents = report(
            home,
            Query {
                group: Some("agent".into()),
                agent: Some("subagent".into()),
                ..query
            },
        )
        .unwrap();
        assert_eq!(agents.totals.input, 900);
        assert_eq!(own.totals.input + agents.totals.input, family.totals.input);
    }

    #[test]
    fn chat_consumption_keeps_settled_usage_without_timing_and_excludes_inheritance() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("rollout-main.jsonl");
        let usage = |thread: &str, turn: &str, response: &str, input: u64, output: u64| serde_json::json!({"type":"token_usage_record","payload":{"thread_id":thread,"turn_id":turn,"response_id":response,"usage":{"input_tokens":input,"cached_input_tokens":input-10,"output_tokens":output,"reasoning_output_tokens":0}}});
        let records = vec![
            serde_json::json!({"type":"session_meta","payload":{"id":"main"}}),
            serde_json::json!({"type":"session_meta","payload":{"id":"parent"}}),
            usage("parent", "old", "parent-request", 9000, 9000),
            serde_json::json!({"type":"session_meta","payload":{"id":"main"}}),
            serde_json::json!({"type":"turn_context","payload":{"thread_id":"main","turn_id":"one","model":"gpt-6.1-sol"}}),
            usage("main", "one", "settled", 100, 10),
            usage("main", "one", "settled", 100, 10),
            usage("child", "one", "foreign", 5000, 5000),
            serde_json::json!({"type":"event_msg","payload":{"type":"turn_aborted","thread_id":"main","turn_id":"one"}}),
            serde_json::json!({"type":"turn_context","payload":{"thread_id":"main","turn_id":"two","model":"unpriced-model"}}),
            usage("main", "two", "current", 200, 20),
        ];
        std::fs::write(
            &path,
            records
                .into_iter()
                .map(|r| format!("{r}\n"))
                .collect::<String>(),
        )
        .unwrap();
        let db = Connection::open(dir.path().join("state_5.sqlite")).unwrap();
        db.execute_batch("CREATE TABLE threads(id TEXT,rollout_path TEXT,title TEXT,cwd TEXT,model TEXT,archived INTEGER,created_at INTEGER,updated_at INTEGER);").unwrap();
        db.execute(
            "INSERT INTO threads VALUES('main',?1,'Main',NULL,NULL,0,1,2)",
            [path.to_string_lossy().as_ref()],
        )
        .unwrap();
        let mut data = metrics::read_own(dir.path(), "main").unwrap();
        attach_metrics(&mut data);
        let own = data.consumption.as_ref().unwrap();
        assert_eq!(own.totals.requests, 2);
        assert_eq!(own.totals.input, 300);
        assert_eq!(own.totals.output, 30);
        assert_eq!(own.totals.unpriced_requests, 1);
        assert!((own.totals.cost_usd - 0.000129).abs() < 1e-10);
        assert!(data
            .turns
            .iter()
            .flat_map(|t| &t.requests)
            .all(|r| !r.included));
        assert_eq!(
            data.turns
                .iter()
                .find(|t| t.turn_id == "one")
                .unwrap()
                .status,
            "interrupted"
        );
        assert_eq!(
            data.turns
                .iter()
                .find(|t| t.turn_id == "two")
                .unwrap()
                .status,
            "inProgress"
        );
        let before = serde_json::to_value(own).unwrap();
        crate::rollout_log::compress_fixture(&path);
        let mut compressed = metrics::read_own(dir.path(), "main").unwrap();
        attach_metrics(&mut compressed);
        assert_eq!(
            serde_json::to_value(compressed.consumption).unwrap(),
            before
        );
    }

    #[test]
    fn filters_trends_and_request_pages_share_the_same_usage() {
        let dir = tempfile::tempdir().unwrap();
        super::super::context::confirm(dir.path(), Some("test-account")).unwrap();
        let db = database(dir.path()).unwrap();
        for (id, title, project, parent) in [
            ("main", "Main chat", "p", None),
            ("child", "Worker", "p", Some("main")),
            ("other", "Other chat", "q", None),
        ] {
            db.execute(
                "INSERT INTO usage_threads VALUES(?1,'',?2,?3,?3,?4,NULL)",
                params![id, title, project, parent],
            )
            .unwrap();
        }
        for (id, response, at, model, tier) in [
            ("main", "one", 1000, "gpt-6.1-sol", "default"),
            ("child", "two", 3601000, "gpt-6.1-sol", "priority"),
            ("main", "three", 3602000, "gpt-5.4", "priority"),
            ("other", "four", 3603000, "gpt-6.1-sol", "default"),
            ("main", "later", 90000000, "gpt-6.1-sol", "priority"),
        ] {
            let entry = Entry {
                model: Some(model.into()),
                tier: Some(tier.into()),
                input: 100,
                cached: 90,
                output: 10,
                reasoning: Some(5),
                writes: Some(0),
            };
            db.execute(
                "INSERT INTO usage_requests VALUES(?1,?2,?3,?4)",
                params![id, response, at, serde_json::to_string(&entry).unwrap()],
            )
            .unwrap();
        }
        let query = Query {
            from: Some(0),
            to: Some(7200000),
            group: Some("model".into()),
            project_id: Some("p".into()),
            ..Default::default()
        };
        let result = report(dir.path(), query.clone()).unwrap();
        assert_eq!(result.rows.len(), 2);
        assert_eq!(result.totals.requests, 3);
        assert_eq!(result.totals.subagent_requests, 1);
        assert_eq!(result.totals.conversations, 1);
        let separated = report(
            dir.path(),
            Query {
                ranges: Some(vec![
                    Range {
                        from: 0,
                        to: Some(2000),
                    },
                    Range {
                        from: 3602000,
                        to: Some(3603000),
                    },
                ]),
                ..query.clone()
            },
        )
        .unwrap();
        assert_eq!(
            separated.totals.requests, 2,
            "unselected cycle gaps are excluded"
        );
        assert_eq!(separated.totals.subagent_requests, 0);
        assert_eq!(
            separated
                .trend
                .iter()
                .map(|p| p.totals.requests)
                .sum::<usize>(),
            2
        );
        let overlapping = report(
            dir.path(),
            Query {
                ranges: Some(vec![
                    Range {
                        from: 0,
                        to: Some(7200000),
                    },
                    Range {
                        from: 1000,
                        to: Some(7200000),
                    },
                ]),
                ..query.clone()
            },
        )
        .unwrap();
        assert_eq!(
            overlapping.totals.requests, 3,
            "overlapping selected intervals never duplicate requests"
        );
        assert!(Query {
            ranges: Some(vec![]),
            ..Default::default()
        }
        .validate()
        .is_err());
        assert_eq!(
            result
                .trend
                .iter()
                .map(|p| p.totals.requests)
                .sum::<usize>(),
            3
        );
        assert!(
            (result.trend.iter().map(|p| p.totals.cost_usd).sum::<f64>() - result.totals.cost_usd)
                .abs()
                < 1e-10
        );
        assert_eq!(
            result.projects.len(),
            2,
            "facets describe the range before filters"
        );
        let fast = report(
            dir.path(),
            Query {
                fast_only: Some(true),
                model: Some("gpt-6.1-sol".into()),
                ..query.clone()
            },
        )
        .unwrap();
        assert_eq!(fast.totals.requests, 1);
        assert_eq!(fast.totals.subagent_requests, 1);
        let main = report(
            dir.path(),
            Query {
                agent: Some("main".into()),
                search: Some("MAIN CHAT".into()),
                ..query.clone()
            },
        )
        .unwrap();
        assert_eq!(main.totals.requests, 2);
        assert_eq!(main.totals.subagent_requests, 0);
        let first = report(
            dir.path(),
            Query {
                group: Some("request".into()),
                limit: Some(1),
                ..query.clone()
            },
        )
        .unwrap();
        let next = report(
            dir.path(),
            Query {
                group: Some("request".into()),
                limit: Some(1),
                offset: Some(1),
                ..query.clone()
            },
        )
        .unwrap();
        assert_eq!(first.total_rows, 3);
        assert_eq!(first.rows.len(), 1);
        assert!(first.rows[0].at > next.rows[0].at);
        assert_eq!(
            first.totals.requests, 3,
            "pagination does not change the summary"
        );
        let agents = report(
            dir.path(),
            Query {
                group: Some("agent".into()),
                ..query.clone()
            },
        )
        .unwrap();
        assert_eq!(agents.rows.len(), 2);
        assert_eq!(agents.rows.iter().filter(|r| r.is_subagent).count(), 1);
        assert_eq!(
            report(
                dir.path(),
                Query {
                    source_thread_id: Some("child".into()),
                    ..query
                }
            )
            .unwrap()
            .totals
            .requests,
            1
        );
        assert!(Query {
            limit: Some(0),
            ..Default::default()
        }
        .validate()
        .is_err());
    }

    #[test]
    fn reports_survive_reopening_and_only_changed_days_expire() {
        let dir = tempfile::tempdir().unwrap();
        super::super::context::confirm(dir.path(), Some("test-account")).unwrap();
        let db = database(dir.path()).unwrap();
        db.execute(
            "INSERT INTO usage_threads VALUES('main','','Main','p','Project',NULL,NULL)",
            [],
        )
        .unwrap();
        let entry = serde_json::to_string(&Entry {
            model: Some("gpt-5.4".into()),
            tier: None,
            input: 100,
            cached: 90,
            output: 20,
            reasoning: Some(5),
            writes: None,
        })
        .unwrap();
        db.execute(
            "INSERT INTO usage_requests VALUES('main','one',1000,?1)",
            [&entry],
        )
        .unwrap();
        let query = Query {
            from: Some(0),
            to: Some(86400000),
            ..Default::default()
        };
        let mut first = report(dir.path(), query.clone()).unwrap();
        assert_eq!(first.totals.requests, 1);
        first.rows[0].label = "persisted report".into();
        db.execute(
            "UPDATE usage_reports SET data=?1",
            [serde_json::to_string(&first).unwrap()],
        )
        .unwrap();
        db.execute(
            "INSERT INTO usage_requests VALUES('main','tomorrow',86401000,?1)",
            [&entry],
        )
        .unwrap();
        drop(db);
        assert_eq!(
            report(dir.path(), query.clone()).unwrap().rows[0].label,
            "persisted report"
        );
        let db = database(dir.path()).unwrap();
        db.execute(
            "INSERT INTO usage_requests VALUES('main','two',2000,?1)",
            [&entry],
        )
        .unwrap();
        assert_eq!(
            report(dir.path(), query.clone()).unwrap().totals.requests,
            2
        );
        assert_ne!(
            report(dir.path(), query).unwrap().rows[0].label,
            "persisted report"
        );
    }
    #[test]
    fn current_names_replace_prompt_titles_and_invalidate_cached_reports_without_log_reads() {
        let dir = tempfile::tempdir().unwrap();
        super::super::context::confirm(dir.path(), Some("title-test")).unwrap();
        let catalog = Connection::open(dir.path().join("state_5.sqlite")).unwrap();
        catalog.execute_batch("CREATE TABLE threads(id TEXT,title TEXT,name TEXT,rollout_path TEXT,created_at INTEGER,updated_at INTEGER,thread_source TEXT);
          INSERT INTO threads VALUES('main','First user message','Actual chat title','missing.jsonl',1,2,'user');").unwrap();
        assert_eq!(
            candidates(dir.path(), &Query::default()).unwrap()[0].1,
            "Actual chat title"
        );
        let db = database(dir.path()).unwrap();
        db.execute("INSERT INTO usage_threads VALUES('main','','First user message','p','Project',NULL,NULL)", []).unwrap();
        let entry = Entry {
            model: Some("gpt-5.4".into()),
            tier: None,
            input: 100,
            cached: 90,
            output: 20,
            reasoning: Some(5),
            writes: None,
        };
        db.execute(
            "INSERT INTO usage_requests VALUES('main','one',1000,?1)",
            [serde_json::to_string(&entry).unwrap()],
        )
        .unwrap();
        let query = Query {
            group: Some("chat".into()),
            ..Default::default()
        };
        assert_eq!(
            report(dir.path(), query.clone()).unwrap().rows[0].label,
            "Actual chat title"
        );
        catalog
            .execute("UPDATE threads SET name='Renamed chat'", [])
            .unwrap();
        let renamed = report(dir.path(), query.clone()).unwrap();
        assert_eq!(renamed.rows[0].label, "Renamed chat");
        assert_eq!(renamed.totals.input, 100);
        catalog
            .execute("UPDATE threads SET name='   '", [])
            .unwrap();
        assert_eq!(
            report(dir.path(), query).unwrap().rows[0].label,
            "First user message"
        );
    }
    #[test]
    fn indexes_only_the_selected_period_until_all_is_requested() {
        let dir = tempfile::tempdir().unwrap();
        super::super::context::confirm(dir.path(), Some("test-account")).unwrap();
        let catalog = Connection::open(dir.path().join("state_5.sqlite")).unwrap();
        catalog.execute_batch("CREATE TABLE threads(id TEXT,title TEXT,rollout_path TEXT,created_at INTEGER,updated_at INTEGER,thread_source TEXT);
          INSERT INTO threads VALUES('old','Old','missing-old.jsonl',1,2,'user');
          INSERT INTO threads VALUES('current','Current','missing-current.jsonl',100,150,'user');").unwrap();
        let selected = Query {
            from: Some(100000),
            to: Some(200000),
            ..Default::default()
        };
        index(dir.path(), None, &selected, |_, total| assert_eq!(total, 1)).unwrap();
        let db = database(dir.path()).unwrap();
        assert_eq!(
            db.query_row(
                "SELECT count(*) FROM usage_threads WHERE id='old'",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
            0
        );
        index(dir.path(), None, &Query::default(), |_, total| {
            assert_eq!(total, 2)
        })
        .unwrap();
        assert_eq!(
            db.query_row(
                "SELECT count(*) FROM usage_threads WHERE id='old'",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
            1
        );
    }
    #[test]
    fn indexes_compressed_own_usage_and_refreshes_replacements() {
        use serde_json::json;
        let dir = tempfile::tempdir().unwrap();
        super::super::context::confirm(dir.path(), Some("test-account")).unwrap();
        let catalog = Connection::open(dir.path().join("state_5.sqlite")).unwrap();
        catalog.execute_batch("CREATE TABLE threads(id TEXT,title TEXT,rollout_path TEXT,model TEXT,cwd TEXT,archived INTEGER,created_at INTEGER,updated_at INTEGER,thread_source TEXT);").unwrap();
        let path = dir.path().join("main.jsonl");
        let records = vec![
            json!({"type":"session_meta","payload":{"id":"main"}}),
            json!({"type":"session_meta","payload":{"id":"ancestor"}}),
            json!({"timestamp":"2026-10-23T00:00:00Z","type":"token_usage_record","payload":{"thread_id":"ancestor","turn_id":"old","response_id":"inherited","usage":{"input_tokens":99999,"cached_input_tokens":0,"output_tokens":99999}}}),
            json!({"type":"session_meta","payload":{"id":"main"}}),
            json!({"type":"turn_context","payload":{"turn_id":"own","model":"gpt-6.1-sol"}}),
            json!({"timestamp":"2026-10-23T01:00:00Z","type":"token_usage_record","payload":{"thread_id":"main","turn_id":"own","response_id":"own-response","usage":{"input_tokens":100,"cached_input_tokens":90,"output_tokens":10,"reasoning_output_tokens":5,"input_tokens_details":{"cache_write_tokens":0}}}}),
            json!({"type":"event_msg","payload":{"type":"turn_aborted","turn_id":"own"}}),
        ];
        let bytes = records
            .iter()
            .map(|r| format!("{r}\n"))
            .collect::<String>()
            .into_bytes();
        let compressed = path.with_extension("jsonl.zst");
        std::fs::write(
            &compressed,
            zstd::stream::encode_all(bytes.as_slice(), 1).unwrap(),
        )
        .unwrap();
        catalog
            .execute(
                "INSERT INTO threads VALUES('main','Main',?1,'gpt-6.1-sol','C:/p',0,1,2,'user')",
                [path.to_string_lossy().as_ref()],
            )
            .unwrap();
        index(dir.path(), None, &Query::default(), |_, _| {}).unwrap();
        let first = report(dir.path(), Query::default()).unwrap();
        assert_eq!(first.totals.requests, 1, "{:?}", first.warnings);
        assert_eq!(first.totals.input, 100);
        assert_eq!(first.undated_requests, 0);
        index(dir.path(), None, &Query::default(), |_, _| {}).unwrap();
        assert_eq!(
            report(dir.path(), Query::default())
                .unwrap()
                .totals
                .requests,
            1
        );
        let mut replaced = records;
        replaced[5]["payload"]["usage"]["output_tokens"] = json!(20);
        let bytes = replaced
            .iter()
            .map(|r| format!("{r}\n"))
            .collect::<String>()
            .into_bytes();
        std::fs::write(
            &compressed,
            zstd::stream::encode_all(bytes.as_slice(), 1).unwrap(),
        )
        .unwrap();
        index(dir.path(), None, &Query::default(), |_, _| {}).unwrap();
        assert_eq!(
            report(dir.path(), Query::default()).unwrap().totals.output,
            20
        );
    }

    #[test]
    #[ignore = "indexes local usage on demand and records a read-only external quota snapshot"]
    fn live_usage_index_and_quota_snapshot() {
        let home = crate::source::codex_home().unwrap();
        let thread = std::env::var("CODEX_PANE_VERIFY_THREAD").unwrap();
        let limits = crate::source::read_live_rate_limits(Some(&thread)).unwrap();
        assert!(limits.reset_credits.is_some());
        super::super::context::confirm(&home, limits.account_id.as_deref()).unwrap();
        super::super::ledger::record(&home, &limits).unwrap();
        let snapshot = crate::source::scan().unwrap();
        let history = super::super::ledger::history(&home).unwrap();
        let from = history
            .cycles
            .iter()
            .filter(|c| c.ended_at.is_none())
            .map(|c| c.started_at.unwrap_or(c.observed_start))
            .min()
            .unwrap();
        let query = Query {
            from: Some(from),
            ..Default::default()
        };
        for pass in 1..=2 {
            let started = std::time::Instant::now();
            index(&home, Some(&snapshot), &query, |done, total| {
                if done % 100 == 0 || done == total {
                    println!("pass={pass} progress={done}/{total}");
                }
            })
            .unwrap();
            let value = report(&home, query.clone()).unwrap();
            println!("pass={pass} ms={} requests={} conversations={} usd={:.4} unpriced={} warnings={} undated={}",started.elapsed().as_millis(),value.totals.requests,value.totals.conversations,value.totals.cost_usd,value.totals.unpriced_requests,value.warnings.len(),value.undated_requests);
            assert!(value.totals.requests > 0);
        }
    }
    #[test]
    fn question_rounds_are_lazy_cached_filtered_and_refreshed_after_compressed_log_changes() {
        use serde_json::json;
        let dir = tempfile::tempdir().unwrap();
        super::super::context::confirm(dir.path(), Some("test-account")).unwrap();
        let catalog = Connection::open(dir.path().join("state_5.sqlite")).unwrap();
        catalog.execute_batch("CREATE TABLE threads(id TEXT,title TEXT,rollout_path TEXT,created_at INTEGER,updated_at INTEGER,thread_source TEXT,cwd TEXT,model TEXT,archived INTEGER DEFAULT 0);").unwrap();
        let path = dir.path().join("main.jsonl");
        catalog
            .execute(
                "INSERT INTO threads(id,title,rollout_path,created_at,updated_at,thread_source) VALUES('main','Main title',?1,0,10,'user')",
                [path.to_string_lossy().as_ref()],
            )
            .unwrap();
        let mut records = vec![
            json!({"type":"session_meta","payload":{"id":"main"}}),
            json!({"type":"event_msg","payload":{"type":"task_started","turn_id":"one"}}),
            json!({"timestamp":"1970-01-01T00:00:00Z","type":"event_msg","payload":{"type":"user_message","message":"First question"}}),
            json!({"type":"token_usage_record","payload":{"turn_id":"one","response_id":"a"}}),
            json!({"type":"event_msg","payload":{"type":"turn_aborted","turn_id":"one"}}),
            json!({"type":"event_msg","payload":{"type":"task_started","turn_id":"resume"}}),
            json!({"type":"event_msg","payload":{"type":"user_message","message":"Continue"}}),
            json!({"type":"token_usage_record","payload":{"turn_id":"resume","response_id":"b"}}),
            json!({"timestamp":"1970-01-01T00:00:03Z","type":"event_msg","payload":{"type":"task_complete","turn_id":"resume"}}),
            json!({"type":"event_msg","payload":{"type":"task_started","turn_id":"two"}}),
            json!({"timestamp":"1970-01-01T00:00:03.500Z","type":"response_item","payload":{"type":"message","role":"user","content":[{"text":"Second question"}]}}),
            json!({"type":"token_usage_record","payload":{"turn_id":"two","response_id":"c"}}),
            json!({"type":"event_msg","payload":{"type":"turn_aborted","turn_id":"two"}}),
        ];
        let write_log = |records: &[serde_json::Value]| {
            let text = records.iter().map(|v| format!("{v}\n")).collect::<String>();
            std::fs::write(
                path.with_extension("jsonl.zst"),
                zstd::stream::encode_all(text.as_bytes(), 1).unwrap(),
            )
            .unwrap();
        };
        write_log(&records);
        let db = database(dir.path()).unwrap();
        for (id, title, parent) in [
            ("main", "Main title", None),
            ("child", "Worker title", Some("main")),
        ] {
            db.execute(
                "INSERT INTO usage_threads VALUES(?1,'',?2,'p','Project',?3,NULL)",
                params![id, title, parent],
            )
            .unwrap();
        }
        for (id, response, at, input, tier) in [
            ("main", "a", 1000, 100, "default"),
            ("main", "b", 2000, 200, "priority"),
            ("main", "c", 4000, 300, "default"),
            ("child", "child", 2000, 900, "default"),
        ] {
            let entry = Entry {
                model: Some("gpt-6.1-sol".into()),
                tier: Some(tier.into()),
                input,
                cached: input / 2,
                output: input / 10,
                reasoning: Some(input / 20),
                writes: Some(0),
            };
            db.execute(
                "INSERT INTO usage_requests VALUES(?1,?2,?3,?4)",
                params![id, response, at, serde_json::to_string(&entry).unwrap()],
            )
            .unwrap();
        }
        let selection = Query {
            from: Some(0),
            to: Some(5000),
            group: Some("chat".into()),
            ..Default::default()
        };
        let chats = report(dir.path(), selection.clone()).unwrap();
        assert_eq!(chats.rows.len(), 1);
        assert!(chats.rows.iter().any(|r| r.label == "Main title"
            && r.totals.input == 1500
            && r.totals.subagent_requests == 1));
        assert_eq!(
            db.query_row("SELECT count(*) FROM usage_round_details", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            0,
            "chat grouping only reads the existing usage cache"
        );
        let query = Query {
            group: Some("round".into()),
            source_thread_id: Some("main".into()),
            ..selection
        };
        let details = report(dir.path(), query.clone()).unwrap();
        assert_eq!(details.rows.len(), 2);
        assert_eq!(details.rows[0].round_number, Some(2));
        assert_eq!(details.rows[0].label, "Second question");
        assert_eq!(details.rows[0].status.as_deref(), Some("interrupted"));
        assert_eq!(details.rows[1].label, "First question");
        assert_eq!(
            details.rows[1].totals.input, 300,
            "interrupted and resumed internal turns are one question round"
        );
        assert_eq!(details.rows[1].completed_at, Some(3000));
        assert_eq!(details.totals.input, 600);
        assert_eq!(details.totals.subagent_requests, 0);
        assert_eq!(
            details
                .rows
                .iter()
                .map(|r| r.totals.requests)
                .sum::<usize>(),
            3
        );
        let selected = report(
            dir.path(),
            Query {
                ranges: Some(vec![
                    Range {
                        from: 0,
                        to: Some(1500),
                    },
                    Range {
                        from: 3500,
                        to: Some(5000),
                    },
                ]),
                ..query.clone()
            },
        )
        .unwrap();
        assert_eq!(
            selected.totals.input, 400,
            "unselected intervals never leak into round totals"
        );
        let fast = report(
            dir.path(),
            Query {
                fast_only: Some(true),
                ..query.clone()
            },
        )
        .unwrap();
        assert_eq!(
            fast.rows.len(),
            2,
            "a question with no matching filtered usage remains visible"
        );
        assert_eq!(fast.rows[1].round_number, Some(1));
        assert_eq!(fast.totals.input, 200);
        let (mut saved, _) = rounds::read(dir.path(), "main").unwrap();
        saved.rounds[0].prompt = "Persisted question".into();
        db.execute(
            "UPDATE usage_round_details SET data=?1 WHERE thread_id='main'",
            [serde_json::to_string(&saved).unwrap()],
        )
        .unwrap();
        drop(db);
        assert_eq!(
            rounds::read(dir.path(), "main").unwrap().0.rounds[0].prompt,
            "Persisted question",
            "the per-chat boundary cache survives reopening without parsing again"
        );
        records[2]["payload"]["message"] = json!("Changed question after compression");
        write_log(&records);
        let updated = report(dir.path(), query).unwrap();
        assert_eq!(
            updated.rows[1].label, "Changed question after compression",
            "boundary changes invalidate cached reports even when token usage is unchanged"
        );
        assert_eq!(updated.totals.input, 600);
        assert!(Query {
            group: Some("round".into()),
            ..Default::default()
        }
        .validate()
        .is_err());
    }

    #[test]
    fn chat_families_and_all_other_groups_share_the_same_role_totals() {
        let dir = tempfile::tempdir().unwrap();
        super::super::context::confirm(dir.path(), Some("test-account")).unwrap();
        let db = database(dir.path()).unwrap();
        for (id, title, parent, model) in [
            ("main", "Main title", None, "gpt-6.1-sol"),
            ("worker", "Worker title", Some("main"), "gpt-6.1-sol"),
            ("leaf", "Nested worker", Some("worker"), "unpriced-model"),
            ("other", "Other chat", None, "gpt-6.1-sol"),
            ("orphan", "Unknown parent", Some("missing"), "gpt-6.1-sol"),
        ] {
            db.execute(
                "INSERT INTO usage_threads VALUES(?1,'',?2,'p','Project',?3,NULL)",
                params![id, title, parent],
            )
            .unwrap();
            let entry = Entry {
                model: Some(model.into()),
                tier: Some("default".into()),
                input: 100000,
                cached: 90000,
                output: 1000,
                reasoning: Some(500),
                writes: Some(0),
            };
            db.execute(
                "INSERT INTO usage_requests VALUES(?1,'request',1000,?2)",
                params![id, serde_json::to_string(&entry).unwrap()],
            )
            .unwrap();
        }
        for group in ["chat", "model", "project", "day"] {
            let value = report(
                dir.path(),
                Query {
                    group: Some(group.into()),
                    ..Default::default()
                },
            )
            .unwrap();
            assert_eq!(value.totals.requests, 5);
            assert_eq!(value.totals.subagent_requests, 3);
            assert_eq!(value.totals.unpriced_requests, 1);
            assert_eq!(value.totals.subagent_unpriced_requests, 1);
            assert_eq!(
                value.rows.iter().map(|r| r.totals.input).sum::<u64>(),
                500000
            );
            assert_eq!(
                value.rows.iter().map(|r| r.totals.requests).sum::<usize>(),
                5
            );
            assert!(
                (value.totals.main_usd + value.totals.subagent_usd - value.totals.cost_usd).abs()
                    < 1e-9
            );
            assert!(
                (value.rows.iter().map(|r| r.totals.main_usd).sum::<f64>() - value.totals.main_usd)
                    .abs()
                    < 1e-9
            );
            assert!(
                (value
                    .rows
                    .iter()
                    .map(|r| r.totals.subagent_usd)
                    .sum::<f64>()
                    - value.totals.subagent_usd)
                    .abs()
                    < 1e-9
            );
            if group == "chat" {
                assert_eq!(
                    value.rows.len(),
                    3,
                    "known subagents never appear beside their parent"
                );
                let family = value.rows.iter().find(|r| r.key == "main").unwrap();
                assert_eq!(family.label, "Main title");
                assert_eq!(family.thread_id.as_deref(), Some("main"));
                assert_eq!(family.totals.requests, 3);
                assert_eq!(family.totals.subagent_requests, 2);
                assert!(!family.is_subagent);
                assert!(
                    value
                        .rows
                        .iter()
                        .any(|r| r.key == "orphan" && r.is_subagent),
                    "unresolved parents do not discard cached usage"
                );
            }
        }
        let children = report(
            dir.path(),
            Query {
                group: Some("agent".into()),
                thread_id: Some("main".into()),
                agent: Some("subagent".into()),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(
            children.rows.len(),
            2,
            "nested subagents are each listed once with their own cost"
        );
        assert!(children
            .rows
            .iter()
            .any(|r| r.label == "Worker title" && r.totals.requests == 1));
        assert!(children
            .rows
            .iter()
            .any(|r| r.label == "Nested worker" && r.totals.unpriced_requests == 1));
        assert_eq!(children.totals.requests, 2);
        let own = report(
            dir.path(),
            Query {
                group: Some("chat".into()),
                thread_id: Some("main".into()),
                agent: Some("main".into()),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(own.totals.requests, 1);
        assert_eq!(own.totals.subagent_requests, 0);
        let selected = report(
            dir.path(),
            Query {
                group: Some("chat".into()),
                search: Some("Nested worker".into()),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(selected.rows.len(), 1);
        assert_eq!(selected.rows[0].label, "Main title");
        assert_eq!(selected.totals.requests, 1);
        assert_eq!(
            db.query_row("SELECT count(*) FROM usage_round_details", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            0,
            "these summaries never parse round logs"
        );
    }

    #[test]
    fn groups_settled_interrupted_requests_and_children_once() {
        let dir = tempfile::tempdir().unwrap();
        super::super::context::confirm(dir.path(), Some("test-account")).unwrap();
        let db = database(dir.path()).unwrap();
        for (id, parent) in [("main", None), ("child", Some("main"))] {
            db.execute(
                "INSERT INTO usage_threads VALUES(?1,'','Chat','p','Project',?2,NULL)",
                params![id, parent],
            )
            .unwrap();
            let entry = Entry {
                model: Some("gpt-6.1-sol".into()),
                tier: Some("default".into()),
                input: 100000,
                cached: 90000,
                output: 1000,
                reasoning: Some(500),
                writes: Some(0),
            };
            db.execute(
                "INSERT INTO usage_requests VALUES(?1,'r',0,?2)",
                params![id, serde_json::to_string(&entry).unwrap()],
            )
            .unwrap();
        }
        let result = report(
            dir.path(),
            Query {
                group: Some("chat".into()),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(result.rows.len(), 1);
        assert_eq!(result.rows[0].thread_id.as_deref(), Some("main"));
        assert_eq!(result.rows[0].totals.requests, 2);
        assert_eq!(result.totals.requests, 2);
        assert_eq!(result.totals.conversations, 1);
        assert_eq!(result.totals.subagent_requests, 1);
        assert!((result.totals.cost_usd - 0.078).abs() < 1e-9);
        let filtered = report(
            dir.path(),
            Query {
                from: Some(1),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(filtered.totals.requests, 0);
        let project = report(
            dir.path(),
            Query {
                group: Some("project".into()),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(project.rows[0].label, "Project");
    }
}
