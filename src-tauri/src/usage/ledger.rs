use super::{database, now};
use crate::source::RateLimits;
use rusqlite::{params, OptionalExtension};
use serde::Serialize;
use std::{collections::HashMap, path::Path};

#[derive(Clone, Debug, Serialize)]
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

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct History {
    pub cycles: Vec<Cycle>,
    pub observed_since: Option<i64>,
    pub account_known: bool,
    pub limits: Option<RateLimits>,
}

struct Sample {
    id: i64,
    at: i64,
    until: i64,
    value: RateLimits,
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
    let db = database(home)?;
    let account: Option<String> = db
        .query_row(
            "SELECT account FROM quota_samples ORDER BY id DESC LIMIT 1",
            [],
            |r| r.get(0),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    let mut samples = vec![];
    if let Some(account) = &account {
        let mut statement = db
            .prepare("SELECT id,at,until,data FROM quota_samples WHERE account=?1 ORDER BY id")
            .map_err(|e| e.to_string())?;
        let rows = statement
            .query_map([account], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, i64>(2)?,
                    r.get::<_, String>(3)?,
                ))
            })
            .map_err(|e| e.to_string())?;
        for row in rows {
            let (id, at, until, data) = row.map_err(|e| e.to_string())?;
            samples.push(Sample {
                id,
                at,
                until,
                value: serde_json::from_str(&data).map_err(|e| format!("额度历史记录损坏：{e}"))?,
            });
        }
    }
    let mut cycles = build(&samples);
    for cycle in &mut cycles {
        cycle.acknowledged = db
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM quota_ack WHERE event_id=?1)",
                [cycle.notice_id.as_ref().unwrap_or(&cycle.id)],
                |r| r.get(0),
            )
            .map_err(|e| e.to_string())?;
    }
    Ok(History {
        cycles,
        observed_since: samples.first().map(|s| s.at),
        account_known: account.as_deref().is_some_and(|s| s != "unknown"),
        limits: samples.last().map(|s| s.value.clone()),
    })
}

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

fn build(samples: &[Sample]) -> Vec<Cycle> {
    let mut complete = vec![];
    let mut active: HashMap<i64, (Cycle, f64, i64, Option<String>)> = HashMap::new();
    let mut previous: Option<&Sample> = None;
    for sample in samples {
        for window in [&sample.value.primary, &sample.value.secondary]
            .into_iter()
            .flatten()
        {
            let minutes = window.window_minutes;
            if ![300, 10080].contains(&minutes) || !window.used_percent.is_finite() {
                continue;
            }
            let used = window.used_percent.clamp(0.0, 100.0);
            let deadline = window.resets_at.map(|n| n.saturating_mul(1000));
            let plan = sample.value.plan_type.clone();
            let mut start = None;
            let mut incomplete = true;
            if let Some((mut cycle, last_used, last_at, last_plan)) = active.remove(&minutes) {
                let changed_plan = plan != last_plan;
                let elapsed = cycle.scheduled_reset.is_some_and(|at| at <= sample.at);
                let reset = deadline != cycle.scheduled_reset && (used < last_used || elapsed);
                if reset || changed_plan {
                    let boundary = if elapsed && !changed_plan {
                        cycle.scheduled_reset.unwrap()
                    } else {
                        sample.at
                    };
                    cycle.ended_at = Some(boundary);
                    cycle.notice_id = Some(sample.id.to_string());
                    cycle.boundary_from = Some(last_at.min(boundary));
                    cycle.boundary_to = Some(sample.at);
                    cycle.reset_type = Some(
                        if changed_plan {
                            "plan_changed"
                        } else if previous.is_some_and(|old| redeemed(&old.value, &sample.value)) {
                            "card"
                        } else if elapsed {
                            "natural"
                        } else {
                            "early"
                        }
                        .into(),
                    );
                    cycle.outcome = if cycle.exhausted_at.is_some() {
                        "exhausted"
                    } else {
                        "unknown"
                    }
                    .into();
                    cycle.incomplete |= changed_plan || sample.at - last_at > minutes * 60000;
                    complete.push(cycle);
                    start = Some(boundary);
                    incomplete = changed_plan || sample.at - last_at > minutes * 60000;
                } else {
                    cycle.observed_consumed += (used - last_used).max(0.0);
                    cycle.remaining = 100.0 - used;
                    cycle.scheduled_reset = deadline;
                    if used >= 100.0 {
                        cycle.exhausted_at.get_or_insert(sample.at);
                        cycle.outcome = "exhausted".into();
                    }
                    active.insert(minutes, (cycle, used, sample.until, plan));
                    continue;
                }
            }
            active.insert(
                minutes,
                (
                    Cycle {
                        id: format!("{}-{minutes}", sample.id),
                        minutes,
                        started_at: start,
                        observed_start: sample.at,
                        ended_at: None,
                        boundary_from: None,
                        boundary_to: None,
                        scheduled_reset: deadline,
                        reset_type: None,
                        outcome: if used >= 100.0 { "exhausted" } else { "active" }.into(),
                        exhausted_at: (used >= 100.0).then_some(sample.at),
                        remaining: 100.0 - used,
                        observed_consumed: if start.is_some() && !incomplete {
                            used
                        } else {
                            0.0
                        },
                        incomplete,
                        acknowledged: false,
                        notice_id: None,
                    },
                    used,
                    sample.until,
                    plan,
                ),
            );
        }
        previous = Some(sample);
    }
    complete.extend(active.into_values().map(|(cycle, _, _, _)| cycle));
    complete.sort_by_key(|c| std::cmp::Reverse(c.ended_at.unwrap_or(i64::MAX)));
    complete
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
    fn sample(id: i64, at: i64, used: f64, reset: i64) -> Sample {
        Sample {id,at:at*1000,until:at*1000,value:serde_json::from_value(json!({"limit_id":"codex","account_id":"test","plan_type":"plus","primary":{"used_percent":used,"window_minutes":300,"resets_at":reset}})).unwrap()}
    }
    #[test]
    fn exhausted_and_early_cycles_keep_independent_boundaries() {
        let rows = build(&[
            sample(1, 10, 20.0, 100),
            sample(2, 50, 100.0, 100),
            sample(3, 101, 0.0, 400),
            sample(4, 120, 30.0, 400),
            sample(5, 140, 0.0, 500),
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
