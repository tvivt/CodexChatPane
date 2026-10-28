use super::{database, now};
use crate::source::RateLimits;
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::path::Path;

pub(crate) mod logs;
mod recognizer;
pub use logs::import;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Cycle {
    pub id: String,
    pub minutes: i64,
    pub started_at: Option<i64>,
    pub observed_start: i64,
    pub ended_at: Option<i64>,
    pub boundary_from: Option<i64>,
    pub boundary_to: Option<i64>,
    pub scheduled_reset: Option<i64>,
    pub reset_type: Option<String>,
    pub outcome: String,
    pub exhausted_at: Option<i64>,
    pub remaining: f64,
    pub observed_consumed: f64,
    pub incomplete: bool,
    pub acknowledged: bool,
    pub notice_id: Option<String>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct History {
    pub cycles: Vec<Cycle>,
    pub observed_since: Option<i64>,
    pub account_known: bool,
    pub limits: Option<RateLimits>,
    pub history_before: Option<i64>,
    pub warnings: Vec<String>,
}

#[derive(Clone, Serialize, Deserialize)]
struct Sample {
    id: i64,
    at: i64,
    until: i64,
    value: RateLimits,
    #[serde(default)]
    account_boundary: bool,
}

pub fn record(home: &Path, value: &RateLimits) -> Result<(), String> {
    let db = database(home)?;
    let account = value.account_id.as_deref().unwrap_or("unknown");
    let data = serde_json::to_string(value).map_err(|e| e.to_string())?;
    let mut db = db;
    let tx = db.transaction().map_err(|e| e.to_string())?;
    let previous: Option<(i64, String, String)> = tx
        .query_row(
            "SELECT id,data,account FROM quota_samples ORDER BY id DESC LIMIT 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    if let Some((id, _, _)) =
        previous.filter(|(_, old, old_account)| old == &data && old_account == account)
    {
        tx.execute(
            "UPDATE quota_samples SET until=?1 WHERE id=?2",
            params![now(), id],
        )
    } else {
        tx.execute(
            "INSERT INTO quota_samples(at,until,account,data) VALUES(?1,?1,?2,?3)",
            params![now(), account, data],
        )
    }
    .map_err(|e| e.to_string())?;
    tx.commit().map_err(|e| e.to_string())
}

pub fn acknowledge(home: &Path, id: &str) -> Result<(), String> {
    if id.len() > 100 || !id.bytes().all(|b| b.is_ascii_digit() || b == b'-') {
        return Err("重置事件 ID 无效".into());
    }
    database(home)?
        .execute("INSERT OR IGNORE INTO quota_ack(event_id) VALUES(?1)", [id])
        .map(|_| ())
        .map_err(|e| e.to_string())
}

pub fn history(home: &Path) -> Result<History, String> {
    history_cached(home, true)
}
pub fn history_live(home: &Path) -> Result<History, String> {
    history_cached(home, false)
}

fn history_cached(home: &Path, include_logs: bool) -> Result<History, String> {
    let db = database(home)?;
    let latest: Option<(i64, String, i64)> = db
        .query_row(
            "SELECT id,account,until FROM quota_samples ORDER BY id DESC LIMIT 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    let logs = if include_logs && super::context::identity(home).is_ok() {
        Some(logs::version(home)?)
    } else {
        None
    };
    let ack: i64 = db
        .query_row("SELECT COUNT(*) FROM quota_ack", [], |r| r.get(0))
        .map_err(|e| e.to_string())?;
    let cutoff = logs
        .as_ref()
        .map(|version| version.4.min(latest.as_ref().map_or(0, |row| row.2)));
    let oldest: Option<i64> = if logs.is_some() {
        rusqlite::Connection::open_with_flags(
            home.join("state_5.sqlite"),
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .map_err(|e| e.to_string())?
        .query_row("SELECT MIN(created_at) FROM threads", [], |r| r.get(0))
        .map_err(|e| e.to_string())?
    } else {
        None
    };
    let key = format!(
        "{:?}|{:?}|{:?}|{:?}|{ack}|{cutoff:?}|{oldest:?}|{}",
        home.canonicalize().map_err(|e| e.to_string())?,
        std::fs::metadata(home.join(".codex-chat-pane/usage.sqlite"))
            .ok()
            .and_then(|m| m.created().ok()),
        latest.as_ref().map(|row| (row.0, &row.1, row.2)),
        logs,
        now() / 86_400_000
    );
    let key = format!(
        "{include_logs}|{:?}|{key}",
        super::context::identity(home).ok()
    );
    static CACHE: std::sync::OnceLock<std::sync::Mutex<Vec<(String, History)>>> =
        std::sync::OnceLock::new();
    let cache = CACHE.get_or_init(Default::default);
    {
        let mut values = cache.lock().map_err(|e| e.to_string())?;
        if let Some(index) = values.iter().position(|(old, _)| old == &key) {
            let entry = values.remove(index);
            let result = entry.1.clone();
            values.push(entry);
            return Ok(result);
        }
    }
    let result = read_history(home, include_logs)?;
    let mut values = cache.lock().map_err(|e| e.to_string())?;
    values.push((key, result.clone()));
    if values.len() > 8 {
        values.remove(0);
    }
    Ok(result)
}

fn read_history(home: &Path, include_logs: bool) -> Result<History, String> {
    let db = database(home)?;
    let account: Option<String> = db
        .query_row(
            "SELECT account FROM quota_samples ORDER BY id DESC LIMIT 1",
            [],
            |r| r.get(0),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    let account = super::context::account(home).ok().or(account);
    let mut samples = vec![];
    if let Some(account) = &account {
        let mut statement = db
            .prepare("SELECT id,at,until,data,account FROM quota_samples ORDER BY id")
            .map_err(|e| e.to_string())?;
        let rows = statement
            .query_map([], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, i64>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, String>(4)?,
                ))
            })
            .map_err(|e| e.to_string())?;
        let mut previous_account = None;
        for row in rows {
            let (id, at, until, data, observed_account) = row.map_err(|e| e.to_string())?;
            let account_boundary = previous_account
                .as_ref()
                .is_some_and(|old| old != &observed_account);
            previous_account = Some(observed_account.clone());
            if &observed_account != account {
                continue;
            }
            samples.push(Sample {
                id,
                at,
                until,
                value: {
                    let mut value: RateLimits = serde_json::from_str(&data)
                        .map_err(|e| format!("额度历史记录损坏：{e}"))?;
                    if value.account_id.is_none() && observed_account != "unknown" {
                        value.account_id = Some(observed_account);
                    }
                    value
                },
                account_boundary,
            });
        }
    }
    let account = account.as_deref().unwrap_or("unknown");
    let identity = super::context::identity(home).unwrap_or_else(|_| "unconfirmed".into());
    let live = recognizer::resolve(&db, account, &format!("{identity}|live"), &samples, None)?;
    let live_end = samples.last().map(|s| s.until);
    let limits = samples.last().map(|s| s.value.clone());
    let (history_before, mut warnings) = if include_logs {
        logs::merge(home, live_end, &mut samples)?
    } else {
        (None, vec![])
    };
    samples.sort_by_key(|s| (s.at, std::cmp::Reverse(s.id)));
    let resolved = if include_logs {
        recognizer::resolve(
            &db,
            account,
            &format!("{identity}|history"),
            &samples,
            Some(&live),
        )?
    } else {
        live
    };
    warnings.extend(resolved.warnings());
    if account == "unknown" {
        warnings.push("额度账号未知，观察不能证明跨账号周期连续".into());
    }
    let mut cycles = resolved.cycles();
    for cycle in &mut cycles {
        cycle.incomplete |= account == "unknown";
        cycle.acknowledged = recognizer::historical(&resolved, cycle)
            || db
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM quota_ack WHERE event_id=?1 OR event_id IN (SELECT old_id FROM quota_event_alias WHERE account=?2 AND minutes=?3 AND event_id=?1))",
                    params![cycle.notice_id.as_ref().unwrap_or(&cycle.id), account, cycle.minutes],
                    |r| r.get(0),
                )
                .map_err(|e| e.to_string())?;
    }
    Ok(History {
        cycles,
        observed_since: samples.first().map(|s| s.at),
        account_known: account != "unknown",
        limits,
        history_before,
        warnings,
    })
}

#[cfg(test)]
fn redeemed(before: &RateLimits, after: &RateLimits) -> bool {
    let old = before
        .reset_credits
        .as_ref()
        .and_then(|v| v.credits.as_ref());
    after
        .reset_credits
        .as_ref()
        .and_then(|v| v.credits.as_ref())
        .is_some_and(|rows| {
            rows.iter().any(|credit| {
                matches!(credit.status.as_str(), "redeemed" | "consumed")
                    && old.is_some_and(|rows| {
                        rows.iter()
                            .any(|old| old.id == credit.id && old.status == "available")
                    })
            })
        })
}

#[cfg(test)]
fn build(samples: &[Sample]) -> Vec<Cycle> {
    recognizer::derive(samples).cycles()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn delayed_expired_snapshots_do_not_create_an_early_reset_notice() {
        let rows = build(&[
            sample(1, 10, 80.0, 100),
            sample(2, 110, 80.0, 100),
            sample(3, 120, 0.0, 400),
        ]);
        assert_eq!(
            rows.iter()
                .filter(|c| c.reset_type.as_deref() == Some("natural"))
                .count(),
            1
        );
        assert!(rows
            .iter()
            .all(|c| c.reset_type.as_deref() != Some("early")));
    }
    #[test]
    fn returning_to_a_previous_account_updates_the_visible_partition() {
        let dir = tempfile::tempdir().unwrap();
        let first = sample(1, 10, 20.0, 100).value;
        record(dir.path(), &first).unwrap();
        let mut other = first.clone();
        other.account_id = Some("another".into());
        other.primary.as_mut().unwrap().used_percent = 70.0;
        record(dir.path(), &other).unwrap();
        record(dir.path(), &first).unwrap();
        assert_eq!(history(dir.path()).unwrap().cycles[0].remaining, 80.0);
    }
    #[test]
    fn newly_confirmed_account_cannot_merge_the_previous_accounts_raw_snapshots() {
        let dir = tempfile::tempdir().unwrap();
        record(dir.path(), &sample(1, 10, 30.0, 1000).value).unwrap();
        super::super::context::confirm(dir.path(), Some("another")).unwrap();
        let next = history_live(dir.path()).unwrap();
        assert!(next.cycles.is_empty());
        assert!(next.limits.is_none());
        assert!(next.account_known);
    }
    fn sample(id: i64, at: i64, used: f64, reset: i64) -> Sample {
        Sample {account_boundary:false,id,at:at*1000,until:at*1000,value:serde_json::from_value(json!({"limit_id":"codex","account_id":"test","plan_type":"plus","primary":{"used_percent":used,"window_minutes":300,"resets_at":reset}})).unwrap()}
    }
    fn drifting_idle_samples() -> Vec<Sample> {
        [
            (444,1793169180894,1793169300778,100.0,1793600228),
            (445,1793169361156,1793169361156,0.0,1793774162),
            (446,1793169420991,1793169420991,0.0,1793774221),
            (447,1793169481148,1793169481148,0.0,1793774282),
            (448,1793169550238,1793169550238,0.0,1793774347),
            (449,1793169600789,1793169600789,0.0,1793774401),
            (450,1793169661318,1793169661318,0.0,1793774461),
            (451,1793169720729,1793169720729,0.0,1793774468),
        ].into_iter().map(|(id,at,until,used,reset)| {
            let mut value=sample(id,0,used,reset).value;
            value.primary.as_mut().unwrap().window_minutes=10080;
            value.plan_type=Some("prolite".into());
            value.reset_credits=serde_json::from_value(if id==448 {json!({"availableCount":2})}else{json!({"availableCount":2,"credits":[{"id":"a","status":"available"},{"id":"b","status":"available"}]})}).ok();
            Sample {account_boundary:false,id,at,until,value}
        }).collect()
    }
    #[test]
    fn recovered_idle_deadline_drift_preserves_one_reset_and_its_remaining_quota() {
        let mut samples = drifting_idle_samples();
        let rows = build(&samples);
        assert_eq!(
            rows.len(),
            2,
            "59/61/65 second shifts must not split an unused recovered period"
        );
        let ended = rows.iter().find(|c| c.ended_at.is_some()).unwrap();
        assert_eq!(ended.notice_id.as_deref(), Some("445"));
        assert_eq!(ended.reset_type.as_deref(), Some("early"));
        assert_eq!(ended.boundary_from, Some(1793169300778));
        assert_eq!(ended.ended_at, Some(1793169300778));
        assert_eq!(
            ended.remaining, 0.0,
            "the notice retains the pre-reset exhausted quota"
        );
        let mut value = samples.last().unwrap().value.clone();
        value.primary.as_mut().unwrap().used_percent = 8.0;
        samples.push(Sample {
            account_boundary: false,
            id: 452,
            at: 1793169780729,
            until: 1793169780729,
            value,
        });
        let rows = build(&samples);
        assert_eq!(
            rows.len(),
            2,
            "a stable deadline and first consumption continue the same period"
        );
        assert_eq!(rows[0].id, "445-10080");
        assert_eq!(rows[0].started_at, Some(1793169300778));
        assert_eq!(rows[0].observed_consumed, 8.0);
        assert_eq!(rows[0].remaining, 92.0);
    }
    #[test]
    fn unknown_old_history_cannot_swallow_later_known_account_resets() {
        let live = drifting_idle_samples();
        let mut old = sample(-1781665225942, 1781665225, 95.0, 1781803967);
        old.value.primary.as_mut().unwrap().window_minutes = 10080;
        old.value.account_id = None;
        old.value.observation_source = Some("history-log".into());
        let mut merged = vec![old];
        merged.extend(live);
        let rows = build(&merged);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].started_at, Some(1793169300778));
        assert_eq!(rows[1].notice_id.as_deref(), Some("445"));
        assert_eq!(rows[1].remaining, 0.0);
    }
    #[test]
    fn first_consumption_after_a_sampling_gap_anchors_the_same_idle_period() {
        let mut samples = drifting_idle_samples();
        let mut value = samples.last().unwrap().value.clone();
        value.primary.as_mut().unwrap().used_percent = 1.0;
        samples.push(Sample {
            account_boundary: false,
            id: 452,
            at: 1793169781079,
            until: 1793169781079,
            value,
        });
        assert_eq!(build(&samples).len(), 2);
        samples.retain(|sample| !(447..=451).contains(&sample.id));
        let rows = build(&samples);
        assert_eq!(
            rows.len(),
            2,
            "a sampling gap must not invent a second reset"
        );
        let current = rows.iter().find(|c| c.ended_at.is_none()).unwrap();
        assert_eq!(current.id, "445-10080");
        assert_eq!(current.started_at, Some(1793169300778));
        assert_eq!(current.observed_consumed, 1.0);
        assert_eq!(current.remaining, 99.0);
        let ended = rows.iter().find(|c| c.ended_at.is_some()).unwrap();
        assert_eq!(ended.notice_id.as_deref(), Some("445"));
        assert_eq!(ended.remaining, 0.0);
    }
    #[test]
    fn first_consumption_keeps_independent_reset_evidence() {
        let mut before = sample(1, 10, 0.0, 100);
        before.value.reset_credits = serde_json::from_value(
            json!({"availableCount":1,"credits":[{"id":"card","status":"available"}]}),
        )
        .ok();
        for kind in ["natural", "card", "plan_changed"] {
            let mut after = sample(2, if kind == "natural" { 101 } else { 20 }, 1.0, 400);
            if kind == "card" {
                after.value.reset_credits = serde_json::from_value(
                    json!({"availableCount":0,"credits":[{"id":"card","status":"consumed"}]}),
                )
                .ok();
            } else if kind == "plan_changed" {
                after.value.plan_type = Some("pro".into());
            }
            let rows = build(&[
                Sample {
                    account_boundary: false,
                    id: before.id,
                    at: before.at,
                    until: before.until,
                    value: before.value.clone(),
                },
                Sample {
                    at: 15_000,
                    until: 15_000,
                    id: 10,
                    ..before.clone()
                },
                after,
            ]);
            assert_eq!(rows.len(), 2, "{kind}");
            assert_eq!(rows[1].reset_type.as_deref(), Some(kind));
            assert_eq!(rows[0].remaining, 99.0);
        }
    }
    #[test]
    fn zero_to_zero_deadlines_need_natural_card_or_plan_evidence() {
        let idle = build(&[sample(1, 10, 0.0, 100), sample(2, 20, 0.0, 200)]);
        assert_eq!(idle.len(), 1);
        let natural = build(&[
            sample(1, 10, 0.0, 100),
            sample(3, 20, 0.0, 100),
            sample(2, 101, 0.0, 400),
        ]);
        assert_eq!(
            natural
                .iter()
                .filter(|c| c.reset_type.as_deref() == Some("natural"))
                .count(),
            1
        );
        let mut before = sample(1, 10, 0.0, 100);
        before.value.reset_credits = serde_json::from_value(
            json!({"availableCount":1,"credits":[{"id":"card","status":"available"}]}),
        )
        .ok();
        let mut after = sample(2, 20, 0.0, 200);
        after.value.reset_credits = serde_json::from_value(
            json!({"availableCount":0,"credits":[{"id":"card","status":"consumed"}]}),
        )
        .ok();
        let mut stale = sample(3, 30, 0.0, 100);
        stale.value = before.value.clone();
        let mut next = sample(4, 40, 0.0, 300);
        next.value.reset_credits = after.value.reset_credits.clone();
        let rows = build(&[before, after, stale, next]);
        assert_eq!(
            rows.len(),
            2,
            "stale available cards must not manufacture another redemption"
        );
        assert_eq!(rows[1].reset_type.as_deref(), Some("card"));
        assert_eq!(
            rows[1].remaining, 100.0,
            "a confirmed reset can really have full remaining quota"
        );
        let mut plan = sample(2, 20, 0.0, 200);
        plan.value.plan_type = Some("pro".into());
        assert!(build(&[sample(1, 10, 0.0, 100), plan])
            .iter()
            .any(|c| c.reset_type.as_deref() == Some("plan_changed")));
        let busy = build(&[sample(1, 10, 20.0, 100), sample(2, 20, 30.0, 200)]);
        assert_eq!(
            busy.len(),
            1,
            "higher usage and a new deadline alone cannot prove a reset between polls"
        );
    }
    #[test]
    fn idle_drift_reloads_merge_history_preserve_acknowledgements_and_consumption() {
        for sampling_gap in [false, true] {
            idle_drift_reload_case(sampling_gap);
        }
    }
    fn idle_drift_reload_case(sampling_gap: bool) {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path();
        let path = home.join("main.jsonl");
        let catalog = rusqlite::Connection::open(home.join("state_5.sqlite")).unwrap();
        catalog.execute_batch("CREATE TABLE threads(id TEXT PRIMARY KEY,title TEXT,rollout_path TEXT,cwd TEXT,model TEXT,archived INTEGER,created_at INTEGER,updated_at INTEGER,thread_source TEXT);").unwrap();
        catalog.execute("INSERT INTO threads VALUES('main','Main',?1,'','gpt-6.1-sol',0,1793169000,1793169800,'user')",[path.to_string_lossy().as_ref()]).unwrap();
        super::super::context::confirm(home, Some("test")).unwrap();
        let db = database(home).unwrap();
        let mut samples = drifting_idle_samples();
        if sampling_gap {
            let mut value = samples.last().unwrap().value.clone();
            value.primary.as_mut().unwrap().used_percent = 1.0;
            samples.push(Sample {
                account_boundary: false,
                id: 452,
                at: 1793169781079,
                until: 1793169781079,
                value,
            });
            samples.retain(|sample| !(447..=451).contains(&sample.id));
        }
        for row in &samples {
            db.execute(
                "INSERT INTO quota_samples(id,at,until,account,data) VALUES(?1,?2,?3,'test',?4)",
                params![
                    row.id,
                    row.at,
                    row.until,
                    serde_json::to_string(&row.value).unwrap()
                ],
            )
            .unwrap();
        }
        for id in ["445", "447", "448"] {
            acknowledge(home, id).unwrap();
        }
        let live = history_live(home).unwrap();
        assert_eq!(live.cycles.len(), 2);
        assert!(live
            .cycles
            .iter()
            .any(|c| c.notice_id.as_deref() == Some("445") && c.acknowledged));
        let observations:Vec<logs::Observation>=serde_json::from_value(json!([{"at":1793168921803i64,"until":1793168921803i64,"account":"test","value":samples[0].value}])).unwrap();
        logs::reuse(home, "earlier-observation", "fixture-v1", &observations).unwrap();
        for recovered in [
            history(home).unwrap(),
            history(home).unwrap(),
            read_history(home, true).unwrap(),
        ] {
            assert_eq!(
                recovered.cycles.len(),
                2,
                "import, cache hit and database reload derive the same merged periods"
            );
            let ended = recovered
                .cycles
                .iter()
                .find(|c| c.ended_at.is_some())
                .unwrap();
            assert_eq!(ended.notice_id.as_deref(), Some("445"));
            assert!(ended.acknowledged);
            assert_eq!(ended.remaining, 0.0);
        }
        assert_eq!(
            db.query_row("SELECT COUNT(*) FROM quota_samples", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            samples.len() as i64
        );
        assert_eq!(
            db.query_row(
                "SELECT COUNT(*) FROM quota_ack WHERE event_id IN ('445','447','448')",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
            3,
            "obsolete acknowledgements are kept, not rewritten"
        );
        let mut records = vec![
            json!({"type":"session_meta","payload":{"id":"main"}}),
            json!({"type":"turn_context","payload":{"turn_id":"turn","model":"gpt-6.1-sol"}}),
        ];
        for (id, date) in [
            "2026-10-28T06:35:00.777Z",
            "2026-10-28T06:35:00.778Z",
            "2026-10-28T06:36:30Z",
            "2026-10-28T06:37:30Z",
            "2026-10-28T06:38:30Z",
        ]
        .into_iter()
        .enumerate()
        {
            records.push(json!({"type":"token_usage_record","timestamp":date,"payload":{"thread_id":"main","turn_id":"turn","response_id":format!("request-{id}"),"usage":{"input_tokens":100,"cached_input_tokens":90,"output_tokens":10,"reasoning_output_tokens":0}}}));
        }
        std::fs::write(
            &path,
            records.iter().map(|v| format!("{v}\n")).collect::<String>(),
        )
        .unwrap();
        let current = live.cycles.iter().find(|c| c.ended_at.is_none()).unwrap();
        let query = super::super::costs::Query {
            from: current.started_at,
            to: current.ended_at,
            ..Default::default()
        };
        super::super::costs::index(home, None, &query, |_, _| {}).unwrap();
        let report = super::super::costs::report(home, query).unwrap();
        let report = serde_json::to_value(report).unwrap();
        assert_eq!(report["totals"]["requests"], 4);
        assert_eq!(report["totals"]["input"], 400);
        assert_eq!(
            report["totals"]["output"], 40,
            "requests on both sides of the false old boundaries remain counted exactly once"
        );
        let ended = live.cycles.iter().find(|c| c.ended_at.is_some()).unwrap();
        let all = super::super::costs::Query {
            from: Some(ended.observed_start),
            ..Default::default()
        };
        super::super::costs::index(home, None, &all, |_, _| {}).unwrap();
        let total = serde_json::to_value(super::super::costs::report(home, all).unwrap()).unwrap();
        let before = serde_json::to_value(
            super::super::costs::report(
                home,
                super::super::costs::Query {
                    from: Some(ended.observed_start),
                    to: ended.ended_at,
                    ..Default::default()
                },
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(
            before["totals"]["requests"], 1,
            "settlement immediately before the adopted boundary stays in the preceding period"
        );
        assert_eq!(total["totals"]["requests"], 5);
        for field in ["input", "output", "requests"] {
            assert_eq!(
                before["totals"][field].as_u64().unwrap()
                    + report["totals"][field].as_u64().unwrap(),
                total["totals"][field].as_u64().unwrap()
            );
        }
        let cost = before["totals"]["costUsd"].as_f64().unwrap()
            + report["totals"]["costUsd"].as_f64().unwrap();
        assert!((cost - total["totals"]["costUsd"].as_f64().unwrap()).abs() < 1e-10);
        let multi = super::super::costs::Query {
            ranges: Some(vec![
                super::super::costs::Range {
                    from: ended.observed_start,
                    to: ended.ended_at,
                },
                super::super::costs::Range {
                    from: current.started_at.unwrap(),
                    to: None,
                },
                super::super::costs::Range {
                    from: current.started_at.unwrap(),
                    to: None,
                },
            ]),
            ..Default::default()
        };
        assert_eq!(serde_json::to_value(super::super::costs::report(home,multi).unwrap()).unwrap()["totals"],total["totals"],"adjacent and duplicate selections regroup cached settled requests without duplicate consumption");
    }
    #[test]
    fn obsolete_sliding_notice_dismissal_aliases_the_retained_event_without_deleting_records() {
        let dir = tempfile::tempdir().unwrap();
        let db = database(dir.path()).unwrap();
        for row in drifting_idle_samples() {
            db.execute(
                "INSERT INTO quota_samples(id,at,until,account,data) VALUES(?1,?2,?3,'test',?4)",
                params![
                    row.id,
                    row.at,
                    row.until,
                    serde_json::to_string(&row.value).unwrap()
                ],
            )
            .unwrap();
        }
        acknowledge(dir.path(), "447").unwrap();
        for _ in 0..2 {
            let history = read_history(dir.path(), false).unwrap();
            assert_eq!(history.cycles.len(), 2);
            let ended = history
                .cycles
                .iter()
                .find(|c| c.ended_at.is_some())
                .unwrap();
            assert_eq!(ended.notice_id.as_deref(), Some("445"));
            assert!(ended.acknowledged);
        }
        assert_eq!(
            db.query_row("SELECT count(*) FROM quota_samples", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            8
        );
        assert_eq!(
            db.query_row(
                "SELECT count(*) FROM quota_ack WHERE event_id='447'",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
            1
        );
    }
    #[test]
    fn unchanged_snapshot_confirmation_invalidates_the_history_cache() {
        let dir = tempfile::tempdir().unwrap();
        let db = database(dir.path()).unwrap();
        for row in [sample(1, 10, 80.0, 1000), sample(2, 30, 0.0, 2000)] {
            db.execute(
                "INSERT INTO quota_samples(id,at,until,account,data) VALUES(?1,?2,?3,'test',?4)",
                params![
                    row.id,
                    row.at,
                    row.until,
                    serde_json::to_string(&row.value).unwrap()
                ],
            )
            .unwrap();
        }
        assert_eq!(history_live(dir.path()).unwrap().cycles.len(), 1);
        db.execute("UPDATE quota_samples SET until=40000 WHERE id=2", [])
            .unwrap();
        let confirmed = history_live(dir.path()).unwrap();
        assert_eq!(confirmed.cycles.len(), 2);
        assert_eq!(confirmed.cycles[0].started_at, Some(10_000));
        assert_eq!(confirmed.cycles[1].notice_id.as_deref(), Some("2-300"));
    }
    #[test]
    #[ignore = "replays selected live quota samples read-only; never writes source snapshots or acknowledgements"]
    fn live_idle_deadline_drift_replay() {
        let home = crate::source::codex_home().unwrap();
        let db = rusqlite::Connection::open_with_flags(
            home.join(".codex-chat-pane/usage.sqlite"),
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .unwrap();
        let mut statement=db.prepare("SELECT id,at,until,data FROM quota_samples WHERE id BETWEEN 444 AND 463 ORDER BY id").unwrap();
        let mut samples = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, String>(3)?,
                ))
            })
            .unwrap()
            .map(|row| {
                let (id, at, until, data) = row.unwrap();
                Sample {
                    id,
                    at,
                    until,
                    value: serde_json::from_str(&data).unwrap(),
                    account_boundary: false,
                }
            })
            .collect::<Vec<_>>();
        assert_eq!(samples.len(), 20);
        assert!(samples
            .iter()
            .all(|s| s.value.account_id == samples[0].value.account_id));
        let verify = |name: &str, samples: &[Sample]| {
            let rows = build(samples);
            let resets = rows
                .iter()
                .filter(|c| c.ended_at.is_some() && c.minutes == 10080)
                .collect::<Vec<_>>();
            assert_eq!(resets.len(), 1);
            assert_eq!(resets[0].notice_id.as_deref(), Some("445"));
            assert_eq!(resets[0].remaining, 0.0);
            println!(
                "case={name} samples={} ended_periods={} notice={} boundary_from={:?} remaining={}",
                samples.len(),
                resets.len(),
                resets[0].notice_id.as_deref().unwrap(),
                resets[0].boundary_from,
                resets[0].remaining
            );
        };
        verify("original", &samples[..8]);
        verify("continuous", &samples);
        samples.retain(|sample| !(447..=451).contains(&sample.id));
        verify("sampling-gap", &samples);
    }
    #[test]
    #[ignore = "replays all cached history in an isolated database; original quota/cache databases are read-only"]
    fn live_merged_history_keeps_all_online_resets() {
        use rusqlite::{Connection, OpenFlags};
        let source = crate::source::codex_home().unwrap();
        let raw = Connection::open_with_flags(
            source.join(".codex-chat-pane/usage.sqlite"),
            OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .unwrap();
        let (account, scope): (String, String) = raw
            .query_row(
                "SELECT account,scope FROM quota_derivations WHERE scope LIKE '%|history' LIMIT 1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        let identity = scope.strip_suffix("|history").unwrap();
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path();
        let catalog = Connection::open(home.join("state_5.sqlite")).unwrap();
        catalog.execute_batch("CREATE TABLE threads(id TEXT PRIMARY KEY,created_at INTEGER); INSERT INTO threads VALUES('history',0);").unwrap();
        super::super::context::confirm(home, Some(&account)).unwrap();
        let target = database(home).unwrap();
        let mut statement = raw
            .prepare("SELECT id,at,until,account,data FROM quota_samples ORDER BY id")
            .unwrap();
        let rows = statement
            .query_map([], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, i64>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, String>(4)?,
                ))
            })
            .unwrap();
        for row in rows {
            let (i, a, u, account, data) = row.unwrap();
            target
                .execute(
                    "INSERT INTO quota_samples VALUES(?1,?2,?3,?4,?5)",
                    params![i, a, u, account, data],
                )
                .unwrap();
        }
        let acks = raw
            .prepare("SELECT event_id FROM quota_ack")
            .unwrap()
            .query_map([], |r| r.get::<_, String>(0))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        for id in &acks {
            acknowledge(home, id).unwrap();
        }
        let mut imported = 0;
        for entry in std::fs::read_dir(source.join(".codex-chat-pane")).unwrap() {
            let path = entry.unwrap().path();
            let name = path.file_name().unwrap().to_string_lossy();
            if !name.starts_with("usage-cache-") || !name.ends_with(".sqlite") {
                continue;
            }
            let db = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
            let saved: Option<String> = db
                .query_row("SELECT value FROM cache_identity LIMIT 1", [], |r| r.get(0))
                .optional()
                .unwrap();
            if saved.as_deref() != Some(identity) {
                continue;
            }
            let mut statement = db
                .prepare("SELECT id,signature,data FROM quota_log_files WHERE error IS NULL")
                .unwrap();
            for row in statement
                .query_map([], |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, String>(2)?,
                    ))
                })
                .unwrap()
            {
                let (id, signature, data) = row.unwrap();
                let observations: Vec<logs::Observation> = serde_json::from_str(&data).unwrap();
                logs::reuse(home, &id, &signature, &observations).unwrap();
                imported += 1;
            }
        }
        assert!(imported > 0);
        let live = read_history(home, false).unwrap();
        let merged = read_history(home, true).unwrap();
        let ids: Vec<_> = live
            .cycles
            .iter()
            .filter(|c| c.minutes == 10080 && c.ended_at.is_some())
            .map(|c| c.notice_id.clone().unwrap())
            .collect();
        assert!(ids.iter().any(|id| id == "445"));
        for id in &ids {
            assert!(
                merged
                    .cycles
                    .iter()
                    .any(|c| c.minutes == 10080 && c.notice_id.as_ref() == Some(id)),
                "history lost live event {id}"
            );
        }
        assert!(
            !merged
                .warnings
                .iter()
                .any(|w| w.contains("历史合并未能保留")),
            "{:?}",
            merged.warnings
        );
        let current = merged
            .cycles
            .iter()
            .find(|c| c.minutes == 10080 && c.ended_at.is_none())
            .unwrap();
        assert!(current
            .started_at
            .is_some_and(|at| at >= 1793169180894 && at <= 1793169420991));
        let count: i64 = target
            .query_row("SELECT count(*) FROM quota_ack", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, acks.len() as i64);
        // Reopen resolution from its saved checkpoint as well as the in-memory path.
        let again = read_history(home, true).unwrap();
        assert_eq!(again.cycles.len(), merged.cycles.len());
        println!("files={imported} live_weekly_events={} merged_weekly_events={} current_start={:?} preserved_acknowledgements={count}",ids.len(),merged.cycles.iter().filter(|c|c.minutes==10080&&c.ended_at.is_some()).count(),current.started_at);
    }
    #[test]
    fn exhausted_and_early_cycles_keep_independent_boundaries() {
        let rows = build(&[
            sample(1, 10, 20.0, 100),
            sample(2, 50, 100.0, 100),
            sample(3, 101, 0.0, 400),
            sample(4, 120, 30.0, 400),
            sample(5, 140, 0.0, 500),
            sample(6, 150, 0.0, 500),
        ]);
        assert_eq!(rows.len(), 3);
        let natural = rows
            .iter()
            .find(|c| c.reset_type.as_deref() == Some("natural"))
            .unwrap();
        assert_eq!(natural.exhausted_at, Some(50000));
        assert_eq!(natural.ended_at, Some(100000));
        let early = rows
            .iter()
            .find(|c| c.reset_type.as_deref() == Some("early"))
            .unwrap();
        assert_eq!(early.outcome, "unknown");
        assert_eq!(early.remaining, 70.0);
        assert_eq!(early.boundary_from, Some(120000));
        assert_eq!(early.observed_consumed, 30.0);
    }
    #[test]
    fn disappearing_or_expired_cards_are_not_redemptions() {
        let mut before = sample(1, 10, 50.0, 100).value;
        before.reset_credits = serde_json::from_value(
            json!({"availableCount":1,"credits":[{"id":"c","status":"available","expiresAt":20}]}),
        )
        .ok();
        let mut after = before.clone();
        after.reset_credits = serde_json::from_value(json!({"availableCount":0,"credits":[]})).ok();
        assert!(!redeemed(&before, &after));
        after.reset_credits = serde_json::from_value(
            json!({"availableCount":0,"credits":[{"id":"c","status":"consumed"}]}),
        )
        .ok();
        assert!(redeemed(&before, &after));
    }
    #[test]
    fn unchanged_observations_coalesce_and_dismissals_survive_restart() {
        let dir = tempfile::tempdir().unwrap();
        let value = sample(1, 10, 50.0, 100).value;
        record(dir.path(), &value).unwrap();
        record(dir.path(), &value).unwrap();
        let db = database(dir.path()).unwrap();
        assert_eq!(
            db.query_row("SELECT count(*) FROM quota_samples", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            1
        );
        let first = history(dir.path()).unwrap();
        acknowledge(dir.path(), &first.cycles[0].id).unwrap();
        assert!(history(dir.path()).unwrap().cycles[0].acknowledged);
        let mut other = value;
        other.account_id = Some("other".into());
        record(dir.path(), &other).unwrap();
        assert_eq!(history(dir.path()).unwrap().cycles.len(), 1);
    }
}
