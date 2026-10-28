use super::Sample;
use crate::{
    rollout_log,
    source::RateLimits,
    usage::{context, costs},
};
use rusqlite::{params, Connection, OpenFlags, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    io::{BufRead, BufReader, Read},
    path::Path,
    sync::{Mutex, OnceLock},
};

#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct Observation {
    at: i64,
    until: i64,
    account: Option<String>,
    value: RateLimits,
}

#[derive(Clone, Default, Serialize, Deserialize)]
pub(crate) struct Collector {
    active: String,
    account: Option<String>,
    pub(crate) rows: Vec<Observation>,
}

impl Collector {
    pub(crate) fn apply(&mut self, id: &str, record: &Value) {
        let p = &record["payload"];
        if record["type"] == "session_meta" {
            self.active = p["id"].as_str().unwrap_or("").into();
            self.account = p["creator_account_id"].as_str().map(str::to_owned);
            return;
        }
        let thread = p["thread_id"].as_str();
        if thread.is_some_and(|thread| thread != id) {
            return;
        }
        if thread == Some(id) && self.active != id {
            self.active = id.into();
            self.account = None;
        }
        if self.active != id || record["type"] != "event_msg" || p["type"] != "token_count" {
            return;
        }
        let Some(at) = record["timestamp"]
            .as_str()
            .and_then(|s| {
                time::OffsetDateTime::parse(s, &time::format_description::well_known::Rfc3339).ok()
            })
            .map(|t| (t.unix_timestamp_nanos() / 1_000_000) as i64)
        else {
            return;
        };
        let Ok(value) = serde_json::from_value::<RateLimits>(p["rate_limits"].clone()) else {
            return;
        };
        if value.limit_id.as_deref().is_some_and(|id| id != "codex") {
            return;
        }
        let row = Observation {
            at,
            until: at,
            account: self.account.clone(),
            value,
        };
        if let Some(last) = self.rows.last_mut().filter(|last| {
            last.account == row.account
                && serde_json::to_value(&last.value).ok() == serde_json::to_value(&row.value).ok()
        }) {
            last.until = at;
        } else {
            self.rows.push(row);
        }
    }
}

pub(crate) fn reuse(
    home: &Path,
    id: &str,
    signature: &str,
    rows: &[Observation],
) -> Result<(), String> {
    database(home)?.execute("INSERT INTO quota_log_files VALUES(?1,?2,?3,NULL) ON CONFLICT(id) DO UPDATE SET signature=excluded.signature,data=excluded.data,error=NULL",params![id,signature,serde_json::to_string(rows).map_err(|e|e.to_string())?]).map(|_|()).map_err(|e|e.to_string())
}

fn database(home: &Path) -> Result<Connection, String> {
    let db = context::database(home)?;
    db.execute_batch("CREATE TABLE IF NOT EXISTS quota_log_files(id TEXT PRIMARY KEY,signature TEXT NOT NULL,data TEXT NOT NULL,error TEXT);
        CREATE TABLE IF NOT EXISTS quota_log_range(id INTEGER PRIMARY KEY CHECK(id=1),start INTEGER NOT NULL);
        CREATE TABLE IF NOT EXISTS quota_log_months(start INTEGER PRIMARY KEY);
        CREATE TABLE IF NOT EXISTS quota_log_revision(id INTEGER PRIMARY KEY CHECK(id=1),revision INTEGER NOT NULL);
        INSERT OR IGNORE INTO quota_log_revision VALUES(1,0);
        CREATE TRIGGER IF NOT EXISTS quota_log_insert AFTER INSERT ON quota_log_files BEGIN UPDATE quota_log_revision SET revision=revision+1; END;
        CREATE TRIGGER IF NOT EXISTS quota_log_update AFTER UPDATE OF data,error ON quota_log_files WHEN OLD.data IS NOT NEW.data OR OLD.error IS NOT NEW.error BEGIN UPDATE quota_log_revision SET revision=revision+1; END;
        CREATE TRIGGER IF NOT EXISTS quota_log_delete AFTER DELETE ON quota_log_files BEGIN UPDATE quota_log_revision SET revision=revision+1; END;")
        .map_err(|e| e.to_string())?;
    Ok(db)
}

pub(super) fn version(home: &Path) -> Result<(String, i64, Option<i64>, i64, i64), String> {
    let identity = context::identity(home)?;
    let db = database(home)?;
    let revision: i64 = db
        .query_row(
            "SELECT revision FROM quota_log_revision WHERE id=1",
            [],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())?;
    let start: Option<i64> = db
        .query_row("SELECT MIN(start) FROM quota_log_range", [], |r| r.get(0))
        .map_err(|e| e.to_string())?;
    let months: i64 = db
        .query_row("SELECT COUNT(*) FROM quota_log_months", [], |r| r.get(0))
        .map_err(|e| e.to_string())?;
    static MAXIMA: OnceLock<Mutex<Vec<(String, i64, i64)>>> = OnceLock::new();
    let maxima = MAXIMA.get_or_init(Default::default);
    let saved = maxima
        .lock()
        .map_err(|e| e.to_string())?
        .iter()
        .find(|(key, rev, _)| key == &identity && *rev == revision)
        .map(|row| row.2);
    let max = if let Some(value) = saved {
        value
    } else {
        let value=db.query_row("SELECT COALESCE(MAX(json_extract(observation.value,'$.until')),0) FROM quota_log_files file,json_each(CASE WHEN json_valid(file.data) THEN file.data ELSE '[]' END) observation",[],|r|r.get::<_,i64>(0)).map_err(|e|e.to_string())?;
        let mut saved = maxima.lock().map_err(|e| e.to_string())?;
        saved.retain(|(key, _, _)| key != &identity);
        saved.push((identity.clone(), revision, value));
        if saved.len() > 8 {
            saved.remove(0);
        }
        value
    };
    Ok((identity, revision, start, months, max))
}

fn month_start(at: i64) -> Result<i64, String> {
    let date = time::OffsetDateTime::from_unix_timestamp(at.div_euclid(1000))
        .map_err(|e| e.to_string())?
        .date();
    Ok(time::Date::from_calendar_date(date.year(), date.month(), 1)
        .map_err(|e| e.to_string())?
        .midnight()
        .assume_utc()
        .unix_timestamp()
        * 1000)
}

/// Import one month of quota observations; Token indexing remains range-driven.
pub fn import(home: &Path, before: Option<i64>) -> Result<(), String> {
    let identity = context::identity(home)?;
    if before.is_some_and(|to| to <= 0 || to > super::now().saturating_add(1)) {
        return Err("历史额度时间范围无效".into());
    }
    let from = month_start(before.map(|to| to - 1).unwrap_or_else(super::now))?;
    let key = format!("quota-v1|{identity}|{from}|{before:?}");
    context::share(key, || import_shared(home, before, &identity))
}

fn import_shared(home: &Path, before: Option<i64>, identity: &str) -> Result<(), String> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    let _guard = LOCK
        .get_or_init(Default::default)
        .lock()
        .map_err(|_| "历史额度索引锁已损坏")?;
    if context::identity(home)? != identity {
        return Err("账号或日志来源已切换，停止旧索引".into());
    }
    let now = super::now();
    let to = before.unwrap_or(now.saturating_add(1));
    if to <= 0 || to > now.saturating_add(1) {
        return Err("历史额度时间范围无效".into());
    }
    let from = month_start(if before.is_some() { to - 1 } else { now })?;
    let mut db = database(home)?;
    let query = costs::Query {
        from: Some(from),
        to: Some(to),
        ..Default::default()
    };
    for (id, _, path, _) in costs::candidates(home, &query)? {
        if context::identity(home)? != identity {
            return Err("账号或日志来源已切换，停止旧索引".into());
        }
        let saved: Option<(String, String)> = db
            .query_row(
                "SELECT signature,data FROM quota_log_files WHERE id=?1",
                [&id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
            .map_err(|e| e.to_string())?;
        if saved.is_some_and(|(stamp, data)| {
            costs::unchanged(&stamp, Path::new(&path))
                && serde_json::from_str::<Vec<Observation>>(&data).is_ok()
        }) {
            continue;
        }
        let result = (|| {
            let stamp = costs::signature(home, &id, Path::new(&path))?;
            let rows = read(home, &id, Path::new(&path))?;
            if stamp != costs::signature(home, &id, Path::new(&path))? {
                return Err("读取期间历史日志有更新，请刷新重试".to_string());
            }
            Ok((
                serde_json::to_string(&stamp).map_err(|e| e.to_string())?,
                serde_json::to_string(&rows).map_err(|e| e.to_string())?,
            ))
        })();
        if context::identity(home)? != identity {
            return Err("账号或日志来源已切换，停止旧索引".into());
        }
        match result {
            Ok((stamp, data)) => {
                db.execute("INSERT INTO quota_log_files VALUES(?1,?2,?3,NULL) ON CONFLICT(id) DO UPDATE SET signature=excluded.signature,data=excluded.data,error=NULL", params![id,stamp,data]).map_err(|e| e.to_string())?;
            }
            Err(error) => {
                db.execute("INSERT INTO quota_log_files VALUES(?1,'','[]',?2) ON CONFLICT(id) DO UPDATE SET signature='',error=excluded.error", params![id,error]).map_err(|e| e.to_string())?;
            }
        }
    }
    let tx = db.transaction().map_err(|e| e.to_string())?;
    tx.execute("INSERT INTO quota_log_range VALUES(1,?1) ON CONFLICT(id) DO UPDATE SET start=MIN(start,excluded.start)", [from]).map_err(|e| e.to_string())?;
    tx.execute("INSERT OR IGNORE INTO quota_log_months VALUES(?1)", [from])
        .map_err(|e| e.to_string())?;
    tx.commit().map_err(|e| e.to_string())
}

fn read(home: &Path, id: &str, path: &Path) -> Result<Vec<Observation>, String> {
    let mut collector = Collector::default();
    for (path, end) in crate::preview::metrics_segments(home, id, path)? {
        let file = rollout_log::open(&path)
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
                return Err("历史日志尾行尚未写完".into());
            }
            // Do not deserialize message bodies or tool output just to extract quota snapshots.
            if !line.contains("\"session_meta\"")
                && !line.contains("\"rate_limits\"")
                && !line.contains("\"thread_settings_applied\"")
            {
                continue;
            }
            let record: Value =
                serde_json::from_str(&line).map_err(|e| format!("历史额度日志损坏：{e}"))?;
            collector.apply(id, &record);
        }
    }
    Ok(collector.rows)
}

pub(super) fn merge(
    home: &Path,
    live_end: Option<i64>,
    samples: &mut Vec<Sample>,
) -> Result<(Option<i64>, Vec<String>), String> {
    // Normal quota polling must still work before the live account has been confirmed.
    if context::identity(home).is_err() {
        return Ok((None, Vec::new()));
    }
    let account = context::account(home)?;
    let db = database(home)?;
    let mut warnings = Vec::new();
    let mut unknown = false;
    let mut statement = db
        .prepare("SELECT data,error FROM quota_log_files")
        .map_err(|e| e.to_string())?;
    let rows = statement
        .query_map([], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?))
        })
        .map_err(|e| e.to_string())?;
    let mut failures = 0;
    for row in rows {
        let (data, error) = row.map_err(|e| e.to_string())?;
        failures += usize::from(error.is_some());
        let observations: Vec<Observation> = match serde_json::from_str(&data) {
            Ok(rows) => rows,
            Err(_) => {
                failures += 1;
                continue;
            }
        };
        for row in observations {
            if row.account.as_ref().is_some_and(|a| a != &account)
                || row.value.account_id.as_ref().is_some_and(|a| a != &account)
            {
                continue;
            }
            for at in [row.at, row.until] {
                if live_end.is_some_and(|end| at > end) || at > super::now() {
                    continue;
                }
                unknown |= row.account.is_none() && row.value.account_id.is_none();
                samples.push(Sample {
                    id: -at,
                    at,
                    until: at,
                    value: {
                        let mut value = row.value.clone();
                        value.account_id = value.account_id.or(row.account.clone());
                        value.observation_source = Some("history-log".into());
                        value
                    },
                    account_boundary: false,
                });
            }
        }
    }
    if unknown {
        warnings.push("部分历史快照未记录账号归属，仅代表本机日志".into());
    }
    if failures > 0 {
        warnings.push(format!("{failures} 个历史日志未完整读取，保留已有缓存"));
    }
    let catalog = Connection::open_with_flags(
        home.join("state_5.sqlite"),
        OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .map_err(|e| e.to_string())?;
    let oldest: Option<i64> = catalog
        .query_row("SELECT MIN(created_at)*1000 FROM threads", [], |r| r.get(0))
        .map_err(|e| e.to_string())?;
    let mut before = month_start(super::now())?;
    while db
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM quota_log_months WHERE start=?1)",
            [before],
            |r| r.get::<_, bool>(0),
        )
        .map_err(|e| e.to_string())?
    {
        before = month_start(before - 1)?;
    }
    // The cursor points at the end of the first unscanned month, including gaps after a long absence.
    before = month_start(before + 32 * 86_400_000)?.min(super::now());
    Ok((
        oldest.filter(|oldest| *oldest < before).map(|_| before),
        warnings,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn at(s: &str) -> i64 {
        (time::OffsetDateTime::parse(s, &time::format_description::well_known::Rfc3339)
            .unwrap()
            .unix_timestamp_nanos()
            / 1_000_000) as i64
    }
    fn quota(thread: &str, date: &str, used: f64, reset: &str) -> Value {
        json!({"type":"event_msg","timestamp":date,"payload":{"type":"token_count","thread_id":thread,"rate_limits":{
            "limit_id":"codex","plan_type":"plus","primary":{"used_percent":used,"window_minutes":10080,"resets_at":at(reset)/1000}}}})
    }
    fn usage(thread: &str, response: &str, input: u64) -> Value {
        json!({"type":"token_usage_record","timestamp":"2026-10-04T03:10:00Z","payload":{"thread_id":thread,"turn_id":"turn","response_id":response,
            "usage":{"input_tokens":input,"cached_input_tokens":input/2,"output_tokens":input/10,"reasoning_output_tokens":input/20,"total_tokens":input+input/10}}})
    }
    fn fixture() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        context::confirm(dir.path(), Some("account")).unwrap();
        Connection::open(dir.path().join("state_5.sqlite")).unwrap().execute_batch(
            "CREATE TABLE threads(id TEXT,rollout_path TEXT,title TEXT,cwd TEXT,model TEXT,archived INTEGER,created_at INTEGER,updated_at INTEGER,thread_source TEXT);").unwrap();
        dir
    }
    fn log(
        home: &Path,
        id: &str,
        kind: &str,
        created: &str,
        records: Vec<Value>,
    ) -> std::path::PathBuf {
        let path = home.join(format!("{id}.jsonl"));
        std::fs::write(
            &path,
            records
                .into_iter()
                .map(|r| format!("{r}\n"))
                .collect::<String>(),
        )
        .unwrap();
        Connection::open(home.join("state_5.sqlite"))
            .unwrap()
            .execute(
                "INSERT INTO threads VALUES(?1,?2,?1,NULL,NULL,0,?3,?3,?4)",
                params![id, path.to_string_lossy(), at(created) / 1000, kind],
            )
            .unwrap();
        path
    }
    #[test]
    fn compressed_history_is_cached_and_period_tokens_exclude_inherited_and_duplicate_requests() {
        let dir = fixture();
        let home = dir.path();
        let main = log(
            home,
            "main",
            "user",
            "2026-10-04T03:00:00Z",
            vec![
                json!({"type":"session_meta","payload":{"id":"main","creator_account_id":"account"}}),
                quota("main", "2026-10-04T03:00:00Z", 80.0, "2026-10-10T00:00:00Z"),
                quota("main", "2026-10-04T03:01:00Z", 79.0, "2026-10-10T00:00:05Z"),
                json!({"type":"session_meta","payload":{"id":"parent"}}),
                quota(
                    "parent",
                    "2026-10-04T03:05:00Z",
                    0.0,
                    "2026-10-11T00:00:00Z",
                ),
                usage("parent", "inherited", 9000),
                json!({"type":"session_meta","payload":{"id":"main","creator_account_id":"account"}}),
                json!({"type":"turn_context","payload":{"thread_id":"main","turn_id":"turn","model":"gpt-6.1-sol"}}),
                usage("main", "own", 100),
                usage("main", "own", 100),
                usage("child", "foreign", 9000),
                quota("main", "2026-10-04T18:00:00Z", 10.0, "2026-10-11T18:00:00Z"),
                quota("main", "2026-10-04T18:01:00Z", 10.0, "2026-10-11T18:00:00Z"),
            ],
        );
        log(
            home,
            "child",
            "subagent",
            "2026-10-04T03:00:00Z",
            vec![
                json!({"type":"session_meta","payload":{"id":"child","creator_account_id":"account","thread_source":"subagent","parent_thread_id":"main"}}),
                json!({"type":"session_meta","payload":{"id":"main"}}),
                quota("main", "2026-10-04T03:05:00Z", 0.0, "2026-10-11T00:00:00Z"),
                usage("main", "own", 100),
                json!({"type":"session_meta","payload":{"id":"child","creator_account_id":"account","thread_source":"subagent","parent_thread_id":"main"}}),
                json!({"type":"turn_context","payload":{"thread_id":"child","turn_id":"turn","model":"gpt-6.1-sol"}}),
                usage("child", "worker", 200),
                quota(
                    "child",
                    "2026-10-04T18:00:00Z",
                    10.0,
                    "2026-10-11T18:00:00Z",
                ),
            ],
        );
        let before = Some(at("2026-10-20T00:00:00Z"));
        Connection::open(home.join("state_5.sqlite"))
            .unwrap()
            .execute(
                "UPDATE threads SET updated_at=?1",
                [at("2026-10-04T18:01:00Z") / 1000],
            )
            .unwrap();
        import(home, before).unwrap();
        let history = super::super::history(home).unwrap();
        let cycle = history
            .cycles
            .iter()
            .find(|c| c.reset_type.as_deref() == Some("early"))
            .unwrap();
        assert_eq!(
            history.cycles.len(),
            2,
            "jitter and copied histories cannot invent resets"
        );
        assert_eq!(cycle.ended_at, Some(at("2026-10-04T03:01:00Z")));
        assert!(
            cycle.acknowledged,
            "importing old history never emits a notification"
        );
        let query = costs::Query {
            from: cycle.ended_at,
            to: None,
            group: Some("model".into()),
            ..Default::default()
        };
        costs::index(home, None, &query, |_, _| {}).unwrap();
        let result = serde_json::to_value(costs::report(home, query.clone()).unwrap()).unwrap();
        assert_eq!(result["totals"]["input"], 300);
        assert_eq!(result["totals"]["output"], 30);
        assert_eq!(result["totals"]["subagentRequests"], 1);
        let mut own = query.clone();
        own.agent = Some("main".into());
        assert_eq!(
            serde_json::to_value(costs::report(home, own).unwrap()).unwrap()["totals"]["input"],
            100
        );
        // An unchanged cache survives reopening without rewriting indexed files.
        let db = database(home).unwrap();
        db.execute_batch("CREATE TRIGGER reject_reparse BEFORE UPDATE ON quota_log_files BEGIN SELECT RAISE(FAIL,'unexpected reparse'); END;").unwrap();
        import(home, before).unwrap();
        db.execute_batch("DROP TRIGGER reject_reparse;").unwrap();
        let content = std::fs::read(&main).unwrap();
        std::fs::write(
            main.with_extension("jsonl.zst"),
            zstd::stream::encode_all(content.as_slice(), 1).unwrap(),
        )
        .unwrap();
        std::fs::rename(&main, main.with_extension("saved")).unwrap();
        import(home, before).unwrap();
        costs::index(home, None, &query, |_, _| {}).unwrap();
        assert_eq!(
            serde_json::to_value(costs::report(home, query).unwrap()).unwrap()["totals"],
            result["totals"]
        );
        assert_eq!(super::super::history(home).unwrap().cycles.len(), 2);
        // A failed replacement must preserve previously recovered observations.
        std::fs::write(&main, "{\"type\":\"session_meta\"\n").unwrap();
        import(home, before).unwrap();
        let damaged = super::super::history(home).unwrap();
        assert_eq!(damaged.cycles.len(), 2);
        assert!(damaged.warnings.iter().any(|w| w.contains("未完整读取")));
    }

    #[test]
    fn months_are_lazy_and_known_other_accounts_are_excluded() {
        let dir = fixture();
        let home = dir.path();
        log(
            home,
            "oct",
            "user",
            "2026-10-04T03:00:00Z",
            vec![
                json!({"type":"session_meta","payload":{"id":"oct"}}),
                quota("oct", "2026-10-04T03:00:00Z", 50.0, "2026-10-10T00:00:00Z"),
            ],
        );
        log(
            home,
            "sep",
            "user",
            "2026-09-04T03:00:00Z",
            vec![
                json!({"type":"session_meta","payload":{"id":"sep","creator_account_id":"account"}}),
                quota("sep", "2026-09-04T03:00:00Z", 50.0, "2026-09-10T00:00:00Z"),
            ],
        );
        log(
            home,
            "other",
            "user",
            "2026-10-04T03:00:00Z",
            vec![
                json!({"type":"session_meta","payload":{"id":"other","creator_account_id":"other-account"}}),
                quota("other", "2026-10-04T04:00:00Z", 0.0, "2026-10-11T00:00:00Z"),
            ],
        );
        import(home, Some(at("2026-10-20T00:00:00Z"))).unwrap();
        let h = super::super::history(home).unwrap();
        assert_eq!(h.cycles.len(), 1);
        assert_eq!(h.observed_since, Some(at("2026-10-04T03:00:00Z")));
        assert_eq!(h.history_before, Some(at("2026-10-01T00:00:00Z")));
        assert!(h.warnings.iter().any(|w| w.contains("账号归属")));
        assert_eq!(
            database(home)
                .unwrap()
                .query_row(
                    "SELECT COUNT(*) FROM quota_log_files WHERE id='sep'",
                    [],
                    |r| r.get::<_, i64>(0)
                )
                .unwrap(),
            0
        );
        import(home, h.history_before).unwrap();
        let h = super::super::history(home).unwrap();
        assert_eq!(h.observed_since, Some(at("2026-09-04T03:00:00Z")));
        assert_eq!(h.history_before, None);
        context::confirm(home, Some("other-account")).unwrap();
        assert!(
            super::super::history(home).unwrap().cycles.is_empty(),
            "switching account cannot reuse another cache"
        );
    }

    #[test]
    fn cached_older_months_do_not_hide_an_unscanned_gap() {
        let dir = fixture();
        let home = dir.path();
        log(
            home,
            "old",
            "user",
            "2026-01-04T03:00:00Z",
            vec![
                json!({"type":"session_meta","payload":{"id":"old"}}),
                quota("old", "2026-01-04T03:00:00Z", 50.0, "2026-01-10T00:00:00Z"),
            ],
        );
        import(home, Some(at("2026-02-01T00:00:00Z"))).unwrap();
        import(home, None).unwrap();
        let current = month_start(super::super::now()).unwrap();
        assert_eq!(
            super::super::history(home).unwrap().history_before,
            Some(current)
        );
        import(home, Some(current)).unwrap();
        assert_eq!(
            super::super::history(home).unwrap().history_before,
            Some(month_start(current - 1).unwrap())
        );
    }

    #[test]
    fn fills_gaps_after_live_tracking_started_without_resurrecting_notices() {
        let dir = fixture();
        let home = dir.path();
        log(
            home,
            "gaps",
            "user",
            "2026-10-04T00:00:00Z",
            vec![
                json!({"type":"session_meta","payload":{"id":"gaps","creator_account_id":"account"}}),
                quota("gaps", "2026-10-04T05:00:00Z", 0.0, "2026-10-11T00:00:00Z"),
                quota("gaps", "2026-10-04T05:01:00Z", 0.0, "2026-10-11T00:00:00Z"),
                quota("gaps", "2026-10-04T17:00:00Z", 30.0, "2026-10-11T00:00:00Z"),
                quota("gaps", "2026-10-04T18:00:00Z", 0.0, "2026-10-12T00:00:00Z"),
                quota("gaps", "2026-10-04T18:01:00Z", 0.0, "2026-10-12T00:00:00Z"),
                quota("gaps", "2026-10-04T19:00:00Z", 0.0, "2026-10-11T00:00:00Z"),
            ],
        );
        let ledger = crate::usage::database(home).unwrap();
        for (date, used, reset) in [
            ("2026-10-04T01:00:00Z", 80.0, "2026-10-10T00:00:00Z"),
            ("2026-10-04T21:00:00Z", 20.0, "2026-10-12T00:00:00Z"),
            ("2026-10-04T21:01:00Z", 20.0, "2026-10-12T00:00:00Z"),
        ] {
            let q = quota("gaps", date, used, reset);
            ledger
                .execute(
                    "INSERT INTO quota_samples(at,until,account,data) VALUES(?1,?1,'account',?2)",
                    params![at(date), q["payload"]["rate_limits"].to_string()],
                )
                .unwrap();
        }
        let original = super::super::history(home).unwrap();
        let notice = original
            .cycles
            .iter()
            .find_map(|c| c.notice_id.clone())
            .unwrap();
        super::super::acknowledge(home, &notice).unwrap();
        import(home, Some(at("2026-10-20T00:00:00Z"))).unwrap();
        let recovered = super::super::history(home).unwrap();
        assert_eq!(
            recovered.cycles.len(),
            3,
            "logs fill resets missed while the app was closed and discard stale deadlines"
        );
        assert!(recovered
            .cycles
            .iter()
            .filter(|c| c.ended_at.is_some())
            .all(|c| c.acknowledged));
        assert!(
            recovered
                .cycles
                .iter()
                .any(|c| c.notice_id.as_ref() == Some(&notice)),
            "persisted dismissals retain their identity"
        );
        assert_eq!(
            recovered.limits.unwrap().primary.unwrap().used_percent,
            20.0,
            "historical imports cannot replace current quota/card tips"
        );
    }

    #[test]
    #[ignore = "reads one real local log without writing to the source or app cache"]
    fn live_compressed_history_sample() {
        let home = crate::source::codex_home().unwrap();
        let id = std::env::var("CODEX_PANE_HISTORY_VERIFY_THREAD").unwrap();
        let db = Connection::open_with_flags(
            home.join("state_5.sqlite"),
            OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .unwrap();
        let path: String = db
            .query_row("SELECT rollout_path FROM threads WHERE id=?1", [&id], |r| {
                r.get(0)
            })
            .unwrap();
        let started = std::time::Instant::now();
        let rows = read(&home, &id, Path::new(&path)).unwrap();
        println!(
            "historical snapshots={} first={:?} last={:?} elapsed={:?}",
            rows.len(),
            rows.first().map(|r| r.at),
            rows.last().map(|r| r.until),
            started.elapsed()
        );
        assert!(!rows.is_empty());
        let own = crate::metrics::read_own(&home, &id).unwrap();
        let requests = own.turns.iter().flat_map(|t| &t.requests).count();
        println!(
            "own requests={requests} input={:?} output={:?}",
            own.summary.input_tokens, own.summary.output_tokens
        );
        assert!(requests > 0);
    }

    #[test]
    #[ignore = "recovers the current month into the app cache using read-only external quota and local logs"]
    fn live_month_backfill_and_period_tokens() {
        let home = crate::source::codex_home().unwrap();
        let thread = std::env::var("CODEX_PANE_HISTORY_VERIFY_THREAD").unwrap();
        let limits = crate::source::read_live_rate_limits(Some(&thread)).unwrap();
        context::confirm(&home, limits.account_id.as_deref()).unwrap();
        super::super::record(&home, &limits).unwrap();
        let started = std::time::Instant::now();
        import(&home, None).unwrap();
        let h = super::super::history(&home).unwrap();
        println!("month import elapsed={:?} cycles={} observed_since={:?} older_before={:?} warnings={:?}",started.elapsed(),h.cycles.len(),h.observed_since,h.history_before,h.warnings);
        let cycle = h
            .cycles
            .iter()
            .rev()
            .find(|c| c.minutes == 10080 && c.ended_at.is_some())
            .unwrap();
        let query = costs::Query {
            from: Some(cycle.started_at.unwrap_or(cycle.observed_start)),
            to: cycle.ended_at,
            group: Some("model".into()),
            ..Default::default()
        };
        costs::index(&home, None, &query, |_, _| {}).unwrap();
        let report = serde_json::to_value(costs::report(&home, query).unwrap()).unwrap();
        println!(
            "sample historical period from={:?} to={:?} totals={} warnings={}",
            cycle.started_at.or(Some(cycle.observed_start)),
            cycle.ended_at,
            report["totals"],
            report["warnings"]
        );
        assert!(report["totals"]["requests"].as_u64().unwrap() > 0);
        let warm = std::time::Instant::now();
        import(&home, None).unwrap();
        println!("warm month import elapsed={:?}", warm.elapsed());
    }
}
