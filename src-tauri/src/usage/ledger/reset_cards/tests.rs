use super::*;
use crate::usage::ledger::{self, recognizer, Sample};
use serde_json::json;

fn at(value: &str) -> i64 {
    (time::OffsetDateTime::parse(value, &time::format_description::well_known::Rfc3339)
        .unwrap()
        .unix_timestamp_nanos()
        / 1_000_000) as i64
}
fn auth(home: &Path, account: &str, user: &str) {
    let jwt = format!(
        "header.{}.signature",
        URL_SAFE_NO_PAD.encode(json!({"sub":user}).to_string())
    );
    std::fs::write(
        home.join("auth.json"),
        json!({"tokens":{"access_token":jwt,"account_id":account}}).to_string(),
    )
    .unwrap();
    context::confirm(home, Some(account)).unwrap();
}
fn fixture() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    auth(dir.path(), "account", "user");
    dir
}
fn samples(cards: bool) -> Vec<Sample> {
    let start = at("2026-10-29T15:31:01Z");
    [(-60_000,100.0),(0,100.0),(282_000,0.0),(342_000,0.0)].into_iter().enumerate().map(|(i,(offset,used))| {
        let credits = if cards { Some(json!({"availableCount":if i<2 {2}else{1},"credits":if i<2 {json!([
            {"id":"keep","status":"available","expiresAt":start/1000+86400},
            {"id":"spent","status":"available","expiresAt":start/1000+3600}
        ])}else{json!([{ "id":"keep","status":"available","expiresAt":start/1000+86400}])}})) } else {None};
        Sample { id:573+i as i64,at:start+offset,until:start+offset,account_boundary:false,value:serde_json::from_value(json!({"account_id":"account","observation_source":"online-mcp","primary":{"used_percent":used,"window_minutes":10080,"resets_at":start/1000+604800+if i<2{0}else{282}},"reset_credits":credits})).unwrap() }
    }).collect()
}
fn page(kind: &str, cursor: Option<&str>) -> Page {
    serde_json::from_value(json!({"events":[{"id":"event-used","kind":kind,"occurred_at":"2026-10-29T15:35:13.355956Z"}],"next_cursor":cursor})).unwrap()
}
fn insert_samples(home: &Path, values: &[Sample]) {
    let db = database(home).unwrap();
    for sample in values {
        db.execute(
            "INSERT INTO quota_samples VALUES(?1,?2,?3,'account',?4)",
            params![
                sample.id,
                sample.at,
                sample.until,
                serde_json::to_string(&sample.value).unwrap()
            ],
        )
        .unwrap();
    }
}
fn ended(state: &recognizer::State) -> Vec<super::super::Cycle> {
    state
        .cycles()
        .into_iter()
        .filter(|c| c.ended_at.is_some())
        .collect()
}

#[test]
fn server_pages_deduplicate_used_and_granted_and_preserve_late_notice_identity() {
    let dir = fixture();
    let home = dir.path();
    let values = samples(false);
    insert_samples(home, &values);
    let original = ledger::history_live(home).unwrap();
    let before = original
        .cycles
        .iter()
        .find(|c| c.ended_at.is_some())
        .unwrap()
        .clone();
    assert_eq!(before.reset_type.as_deref(), Some("early"));
    ledger::acknowledge(home, before.notice_id.as_ref().unwrap()).unwrap();
    let mut calls = 0;
    refresh_with(
        home,
        &values.last().unwrap().value,
        values[3].at,
        |_, cursor| {
            calls += 1;
            if cursor.is_none() {
                Ok(page("used", Some("next")))
            } else {
                assert_eq!(cursor, Some("next"));
                let mut last = page("used", None);
                last.events.push(ServerEvent {
                    id: "event-granted".into(),
                    kind: "granted".into(),
                    occurred_at: "2026-10-28T00:00:00Z".into(),
                });
                Ok(last)
            }
        },
    )
    .unwrap();
    assert_eq!(calls, 2);
    assert_eq!(used(home).unwrap().len(), 1);
    assert_eq!(revision(home).unwrap().unwrap().1, 2);
    let corrected = ledger::history_live(home).unwrap();
    assert_eq!(corrected.cycles.len(), original.cycles.len());
    let after = corrected
        .cycles
        .iter()
        .find(|c| c.notice_id == before.notice_id)
        .unwrap();
    assert_eq!(after.reset_type.as_deref(), Some("card"));
    assert!(after.acknowledged);
    assert_eq!(
        (
            after.id.clone(),
            after.started_at,
            after.ended_at,
            after.boundary_from,
            after.boundary_to
        ),
        (
            before.id.clone(),
            before.started_at,
            before.ended_at,
            before.boundary_from,
            before.boundary_to
        )
    );
    assert!(!refresh_with(
        home,
        &values[3].value,
        values[3].at + MIN_REFRESH,
        |_, _| panic!("unchanged quota must use cache")
    )
    .unwrap());
}

#[test]
fn grants_or_ambiguous_windows_never_create_card_resets() {
    let dir = fixture();
    let values = samples(false);
    let home = dir.path();
    refresh_with(home, &values[3].value, values[3].at, |_, _| {
        Ok(page("granted", None))
    })
    .unwrap();
    assert!(used(home).unwrap().is_empty());
    let mut values = values;
    for sample in &mut values {
        let mut secondary = sample.value.primary.clone().unwrap();
        secondary.window_minutes = 300;
        sample.value.secondary = Some(secondary);
    }
    let cards = vec![CardUse {
        id: "used".into(),
        account: "account".into(),
        at: at("2026-10-29T15:35:13Z"),
    }];
    let db = database(home).unwrap();
    let state = recognizer::resolve(&db, "account", "ambiguous", &values, None, &cards).unwrap();
    assert_eq!(ended(&state).len(), 2);
    assert!(ended(&state)
        .iter()
        .all(|c| c.reset_type.as_deref() == Some("early")));
    let mut other = cards;
    other[0].account = "other".into();
    let state =
        recognizer::resolve(&db, "account", "other", &samples(false), None, &other).unwrap();
    assert_eq!(ended(&state)[0].reset_type.as_deref(), Some("early"));
}

#[test]
fn partial_pages_survive_failure_and_resume_without_hammering_the_api() {
    let dir = fixture();
    let home = dir.path();
    let values = samples(false);
    let time = values[3].at;
    let result = refresh_with(home, &values[3].value, time, |_, cursor| {
        if cursor.is_none() {
            Ok(page("used", Some("tail")))
        } else {
            Err("offline".into())
        }
    });
    assert!(result.is_err());
    assert_eq!(used(home).unwrap().len(), 1);
    assert!(
        !refresh_with(home, &values[3].value, time + 1000, |_, _| panic!(
            "must throttle failed requests"
        ))
        .unwrap()
    );
    refresh_with(home, &values[3].value, time + MIN_REFRESH, |_, cursor| {
        assert_eq!(cursor, Some("tail"));
        Ok(page("used", None))
    })
    .unwrap();
    assert_eq!(used(home).unwrap().len(), 1);
    assert!(!refresh_with(
        home,
        &values[3].value,
        time + 2 * MIN_REFRESH,
        |_, _| panic!("completed history should be cached")
    )
    .unwrap());
}

#[test]
fn account_and_user_switches_discard_inflight_responses_and_isolate_caches() {
    let dir = fixture();
    let home = dir.path();
    let values = samples(false);
    refresh_with(home, &values[3].value, values[3].at, |_, _| {
        Ok(page("used", None))
    })
    .unwrap();
    auth(home, "account", "other-user");
    assert!(used(home).unwrap().is_empty());
    auth(home, "account", "user");
    assert_eq!(used(home).unwrap().len(), 1);
    let result = refresh_with(home, &values[3].value, values[3].at + 86_400_000, |_, _| {
        auth(home, "other", "other-user");
        Ok(page("used", None))
    });
    assert!(result.unwrap_err().contains("已切换"));
    assert!(used(home).unwrap().is_empty());
    assert!(refresh_with(
        home,
        &values[3].value,
        values[3].at + 86_400_000,
        |_, _| panic!("wrong quota account must not send credentials")
    )
    .is_err());
    auth(home, "account", "user");
    assert_eq!(used(home).unwrap().len(), 1);
}

#[test]
fn disappearing_cards_require_complete_unexpired_details_recovery_and_confirmation() {
    let dir = fixture();
    let db = database(dir.path()).unwrap();
    let state = recognizer::resolve(&db, "account", "fallback", &samples(true), None, &[]).unwrap();
    assert_eq!(ended(&state)[0].reset_type.as_deref(), Some("card"));
    for mode in [
        "expired",
        "missing-expiry",
        "missing-details",
        "count-only",
        "multiple-disappearances",
        "no-recovery",
        "unconfirmed",
    ] {
        let mut values = samples(true);
        match mode {
            "expired" | "missing-expiry" => {
                for sample in &mut values[..2] {
                    let card = &mut sample
                        .value
                        .reset_credits
                        .as_mut()
                        .unwrap()
                        .credits
                        .as_mut()
                        .unwrap()[1];
                    card.expires_at = if mode == "expired" {
                        Some(values_time() / 1000)
                    } else {
                        None
                    };
                }
            }
            "missing-details" => values[2].value.reset_credits.as_mut().unwrap().credits = None,
            "count-only" => {
                for sample in &mut values {
                    sample.value.reset_credits.as_mut().unwrap().credits = None;
                }
            }
            "no-recovery" => {
                for sample in &mut values {
                    sample.value.primary.as_mut().unwrap().used_percent = 100.0;
                }
            }
            "multiple-disappearances" => {
                for sample in &mut values[2..] {
                    let cards = sample.value.reset_credits.as_mut().unwrap();
                    cards.available_count = 0;
                    cards.credits = Some(vec![]);
                }
            }
            _ => {
                values.pop();
            }
        }
        let state = recognizer::resolve(&db, "account", mode, &values, None, &[]).unwrap();
        assert!(
            ended(&state)
                .iter()
                .all(|c| c.reset_type.as_deref() != Some("card")),
            "{mode}"
        );
    }
    fn values_time() -> i64 {
        at("2026-10-29T15:35:00Z")
    }
}

#[test]
fn cards_and_quota_may_arrive_one_observation_apart() {
    for card_first in [true, false] {
        let dir = fixture();
        let db = database(dir.path()).unwrap();
        let mut values = samples(true);
        if card_first {
            values[1].value.reset_credits = values[2].value.reset_credits.clone();
        } else {
            let mut delayed = values[2].clone();
            delayed.at += 30_000;
            delayed.until = delayed.at;
            delayed.id = 999;
            values[2].value.reset_credits = values[1].value.reset_credits.clone();
            values.insert(3, delayed);
        }
        let state =
            recognizer::resolve(&db, "account", "asynchronous", &values, None, &[]).unwrap();
        assert_eq!(
            ended(&state)[0].reset_type.as_deref(),
            Some("card"),
            "card_first={card_first}"
        );
    }
}

#[test]
fn cyclic_pagination_and_conflicting_event_ids_preserve_existing_evidence() {
    let dir = fixture();
    let values = samples(false);
    let home = dir.path();
    assert!(
        refresh_with(home, &values[3].value, values[3].at, |_, _| Ok(page(
            "used",
            Some("repeat")
        )))
        .is_err()
    );
    assert_eq!(used(home).unwrap().len(), 1);
    assert!(refresh_with(
        home,
        &values[3].value,
        values[3].at + MIN_REFRESH,
        |_, _| Ok(page("granted", None))
    )
    .is_err());
    assert_eq!(used(home).unwrap().len(), 1);
}

#[test]
#[ignore = "GET server reset-card history and update only the app quota cache; no Codex writes"]
fn live_server_history_corrects_existing_reset_without_moving_boundaries() {
    let home = crate::source::codex_home().unwrap();
    let auth: Value =
        serde_json::from_slice(&std::fs::read(home.join("auth.json")).unwrap()).unwrap();
    let account = auth["tokens"]["account_id"].as_str().unwrap();
    context::confirm(&home, Some(account)).unwrap();
    let db = database(&home).unwrap();
    let data: String = db
        .query_row(
            "SELECT data FROM quota_samples WHERE account=?1 ORDER BY id DESC LIMIT 1",
            [account],
            |r| r.get(0),
        )
        .unwrap();
    let limits: RateLimits = serde_json::from_str(&data).unwrap();
    let before = ledger::history_live(&home).unwrap();
    let merged_before = ledger::history(&home).unwrap();
    refresh(&home, &limits).unwrap();
    let after = ledger::history_live(&home).unwrap();
    let merged_after = ledger::history(&home).unwrap();
    assert_eq!(merged_before.cycles.len(), merged_after.cycles.len());
    for cycle in &merged_before.cycles {
        let current = merged_after
            .cycles
            .iter()
            .find(|c| c.id == cycle.id)
            .unwrap();
        assert_eq!(
            (
                cycle.started_at,
                cycle.ended_at,
                cycle.boundary_from,
                cycle.boundary_to
            ),
            (
                current.started_at,
                current.ended_at,
                current.boundary_from,
                current.boundary_to
            )
        );
    }
    assert_eq!(
        merged_after
            .cycles
            .iter()
            .find(|c| c.notice_id.as_deref() == Some("575"))
            .unwrap()
            .reset_type
            .as_deref(),
        Some("card")
    );
    assert_eq!(before.cycles.len(), after.cycles.len());
    for cycle in &before.cycles {
        let new = after.cycles.iter().find(|c| c.id == cycle.id).unwrap();
        assert_eq!(
            (
                cycle.started_at,
                cycle.ended_at,
                cycle.boundary_from,
                cycle.boundary_to,
                cycle.acknowledged
            ),
            (
                new.started_at,
                new.ended_at,
                new.boundary_from,
                new.boundary_to,
                new.acknowledged
            )
        );
    }
    let target = after
        .cycles
        .iter()
        .find(|c| c.notice_id.as_deref() == Some("575"))
        .unwrap();
    assert_eq!(target.reset_type.as_deref(), Some("card"));
    assert_eq!(
        target.ended_at.map(|value| value / 1000),
        Some(at("2026-10-29T15:31:01Z") / 1000)
    );
    let state: String = db
        .query_row(
            "SELECT state FROM quota_derivations WHERE account=?1 AND scope LIKE '%|live'",
            [account],
            |r| r.get(0),
        )
        .unwrap();
    let state: Value = serde_json::from_str(&state).unwrap();
    assert!(state["events"]
        .as_array()
        .unwrap()
        .iter()
        .any(|event| event["id"] == "575"
            && event["card_ids"]
                .as_array()
                .unwrap()
                .iter()
                .any(|id| id.as_str().unwrap().starts_with("server:"))));
    println!(
        "server_used={} cycles={} target_notice=575 target_type=card boundary_unchanged=true merged_history_verified=true",
        used(&home).unwrap().len(),
        after.cycles.len()
    );
}
