use super::*;
use serde_json::json;

fn sample(id: i64, at: i64, used: f64, deadline: i64) -> Sample {
    Sample {
        id,
        at: at * 1000,
        until: at * 1000,
        account_boundary: false,
        value: serde_json::from_value(json!({
            "account_id":"test","plan_type":"plus","observation_source":"online-mcp",
            "primary":{"used_percent":used,"window_minutes":300,"resets_at":deadline}
        }))
        .unwrap(),
    }
}
fn ended(state: &State) -> Vec<Cycle> {
    state
        .cycles()
        .into_iter()
        .filter(|c| c.ended_at.is_some())
        .collect()
}

#[test]
fn fixed_expiry_requires_a_fresh_window_not_a_clock_or_expired_copy() {
    let mut samples = vec![
        sample(1, 10, 80.0, 100),
        sample(2, 20, 80.0, 100),
        sample(3, 110, 80.0, 100),
    ];
    let state = derive(&samples);
    assert!(ended(&state).is_empty());
    assert!(state.windows[&300].mode == Mode::Fixed);
    let mut expired = samples.clone();
    expired.push(sample(4, 120, 0.0, 90));
    expired.push(sample(5, 130, 0.0, 90));
    assert!(
        ended(&derive(&expired)).is_empty(),
        "an expired different deadline is still not a fresh natural window"
    );
    samples.push(sample(4, 120, 3.0, 400));
    let state = derive(&samples);
    let rows = ended(&state);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].ended_at, Some(100_000));
    assert_eq!(rows[0].boundary_from, Some(110_000));
    assert_eq!(rows[0].boundary_to, Some(120_000));
    assert_eq!(rows[0].remaining, 20.0);
    assert_eq!(state.windows[&300].cycle.started_at, rows[0].ended_at);
}

#[test]
fn floating_mode_uses_time_geometry_even_with_rounded_nonzero_usage() {
    let mut samples = vec![
        sample(1, 10, 1.0, 18010),
        sample(2, 71, 1.0, 18071),
        sample(3, 136, 1.0, 18136),
    ];
    let state = derive(&samples);
    assert!(state.windows[&300].mode == Mode::Floating);
    assert!(ended(&state).is_empty());
    samples.push(sample(4, 20_000, 2.0, 38_000));
    samples.push(sample(5, 20_060, 5.0, 38_000));
    samples.push(sample(6, 20_120, 7.0, 38_000));
    let state = derive(&samples);
    assert!(
        ended(&state).is_empty(),
        "anchoring an idle deadline never creates the missing initial reset"
    );
    assert!(state.windows[&300].mode == Mode::Fixed);
    assert_eq!(state.windows[&300].cycle.started_at, None);
}

#[test]
fn sustained_deadline_only_changes_remain_candidates_and_normal_observation_can_retract_them() {
    let mut samples = vec![
        sample(1, 10, 30.0, 1000),
        sample(2, 20, 30.0, 1000),
        sample(3, 30, 30.0, 1030),
        sample(4, 40, 30.0, 1030),
        sample(5, 50, 30.0, 1030),
    ];
    let state = derive(&samples);
    assert!(ended(&state).is_empty());
    assert_eq!(
        state.windows[&300].last.deadline,
        Some(1_030_000),
        "consistent observations update the current window without creating a reset"
    );
    assert_eq!(
        state.windows[&300].candidate.as_ref().unwrap().reason,
        "deadline-only"
    );
    samples.push(sample(6, 60, 31.0, 1000));
    let state = derive(&samples);
    assert!(ended(&state).is_empty());
    assert!(state.windows[&300].candidate.is_none());
    assert_eq!(state.windows[&300].cycle.remaining, 69.0);
}

#[test]
fn a_pre_expiry_deadline_candidate_does_not_become_a_natural_reset_just_by_persisting() {
    let state = derive(&[
        sample(1, 10, 30.0, 1000),
        sample(2, 20, 30.0, 1000),
        sample(3, 30, 30.0, 1030),
        sample(4, 40, 30.0, 1030),
        sample(5, 1001, 30.0, 1030),
    ]);
    assert!(ended(&state).is_empty());
    assert!(state.windows[&300].candidate.is_some());
    let unknown = derive(&[
        sample(1, 10, 30.0, 1000),
        sample(2, 30, 30.0, 1030),
        sample(3, 40, 30.0, 1030),
        sample(4, 50, 30.0, 1030),
        sample(5, 1010, 0.0, 3000),
        sample(6, 1020, 0.0, 3000),
    ]);
    let events = ended(&unknown);
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].reset_type.as_deref(),Some("early"),"a later recovery uses the newly observed fixed window, never retroactively certifies the original deadline");
}

#[test]
fn recovery_is_confirmed_then_uses_the_last_valid_pre_reset_observation() {
    let mut samples = vec![
        sample(1, 10, 80.0, 1000),
        sample(2, 20, 80.0, 1000),
        sample(3, 30, 10.0, 2000),
    ];
    assert!(ended(&derive(&samples)).is_empty());
    samples.push(sample(4, 40, 12.0, 2000));
    let state = derive(&samples);
    let events = ended(&state);
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].notice_id.as_deref(), Some("3-300"));
    assert_eq!(events[0].ended_at, Some(20_000));
    assert_eq!(events[0].boundary_to, Some(30_000));
    assert_eq!(events[0].remaining, 20.0);
    assert_eq!(state.windows[&300].cycle.started_at, Some(20_000));
    samples.push(sample(5, 8000, 12.0, 2000));
    let state = derive(&samples);
    assert_eq!(state.windows[&300].cycle.started_at, Some(20_000));
}

#[test]
fn interleaved_old_values_missing_fields_and_source_conflicts_do_not_replace_the_valid_window() {
    let mut samples = vec![
        sample(1, 10, 80.0, 1000),
        sample(2, 20, 80.0, 1000),
        sample(3, 30, 0.0, 2000),
        sample(4, 40, 0.0, 2000),
        sample(5, 50, 80.0, 1000),
    ];
    let mut conflict = sample(6, 60, 0.0, 3000);
    conflict.value.observation_source = Some("history-log".into());
    samples.push(conflict);
    let mut missing = sample(7, 70, 0.0, 2000);
    missing.value.primary.as_mut().unwrap().resets_at = None;
    missing.value.plan_type = None;
    samples.push(missing);
    let state = derive(&samples);
    assert_eq!(ended(&state).len(), 1);
    assert_eq!(state.windows[&300].last.deadline, Some(2_000_000));
    assert!(state.windows[&300].candidate.is_some());
    assert_eq!(state.windows[&300].last.plan.as_deref(), Some("plus"));
    samples.push(sample(8, 80, 1.0, 2000));
    let state = derive(&samples);
    assert_eq!(ended(&state).len(), 1);
    assert!(state.windows[&300].candidate.is_none());
}

#[test]
fn a_card_is_independent_of_percentage_and_deadline_threshold_but_needs_a_window() {
    let mut before = sample(1, 10, 0.0, 1000);
    before.value.reset_credits = serde_json::from_value(
        json!({"availableCount":1,"credits":[{"id":"card","status":"available"}]}),
    )
    .ok();
    before.value.secondary = Some(
        serde_json::from_value(json!({"used_percent":50,"window_minutes":10080,"resets_at":8000}))
            .unwrap(),
    );
    let mut after = before.clone();
    after.id = 2;
    after.at = 20_000;
    after.until = 20_000;
    after.value.primary.as_mut().unwrap().resets_at = Some(1001);
    after.value.reset_credits = serde_json::from_value(
        json!({"availableCount":0,"credits":[{"id":"card","status":"consumed"}]}),
    )
    .ok();
    let state = derive(&[before.clone(), after]);
    assert_eq!(
        ended(&state).len(),
        1,
        "one-second window update and same-card proof suffice for the single affected window"
    );
    assert_eq!(ended(&state)[0].minutes, 300);
    assert_eq!(ended(&state)[0].reset_type.as_deref(), Some("card"));
    let mut after = before.clone();
    after.id = 2;
    after.at = 20_000;
    after.until = 20_000;
    after.value.reset_credits =
        serde_json::from_value(json!({"availableCount":0,"credits":[]})).ok();
    assert!(ended(&derive(&[before, after])).is_empty());
}

#[test]
fn an_unused_floating_window_still_accepts_a_scoped_redemption() {
    let mut samples = vec![
        sample(1, 10, 0.0, 18010),
        sample(2, 70, 0.0, 18070),
        sample(3, 130, 0.0, 18130),
        sample(4, 190, 0.0, 18190),
    ];
    for row in &mut samples {
        row.value.reset_credits = serde_json::from_value(
            json!({"availableCount":1,"credits":[{"id":"card","status":"available"}]}),
        )
        .ok();
    }
    assert!(derive(&samples[..3]).windows[&300].mode == Mode::Floating);
    samples[3].value.reset_credits = serde_json::from_value(
        json!({"availableCount":0,"credits":[{"id":"card","status":"redeemed"}]}),
    )
    .ok();
    let state = derive(&samples);
    let rows = ended(&state);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].reset_type.as_deref(), Some("card"));
    assert_eq!(rows[0].remaining, 100.0);
    assert_eq!(state.events[0].card_ids, vec!["card"]);
}

#[test]
fn consistent_cli_fallback_can_confirm_an_event_after_an_mcp_window() {
    for kind in ["early", "natural", "card"] {
        let used = if kind == "card" { 0.0 } else { 80.0 };
        let mut samples = vec![sample(1, 10, used, 100), sample(2, 20, used, 100)];
        let mut first = sample(3, if kind == "natural" { 110 } else { 30 }, 0.0, 2000);
        first.value.observation_source = Some("cli-fallback".into());
        if kind == "card" {
            for row in &mut samples {
                row.value.reset_credits = serde_json::from_value(
                    json!({"availableCount":1,"credits":[{"id":"card","status":"available"}]}),
                )
                .ok();
            }
            first.value.reset_credits = serde_json::from_value(
                json!({"availableCount":0,"credits":[{"id":"card","status":"consumed"}]}),
            )
            .ok();
        }
        let mut confirmed = first.clone();
        confirmed.id = 4;
        confirmed.at += 10_000;
        confirmed.until = confirmed.at;
        samples.push(first);
        assert!(
            ended(&derive(&samples)).is_empty(),
            "first lower-priority source conflict remains pending"
        );
        samples.push(confirmed);
        let state = derive(&samples);
        assert_eq!(ended(&state).len(), 1, "{kind}");
        assert_eq!(ended(&state)[0].reset_type.as_deref(), Some(kind));
        assert_eq!(ended(&state)[0].notice_id.as_deref(), Some("3-300"));
    }
}

#[test]
fn candidate_mode_and_event_identity_survive_reopening_and_history_refinement() {
    let dir = tempfile::tempdir().unwrap();
    let samples = vec![
        sample(1, 10, 80.0, 1000),
        sample(2, 20, 80.0, 1000),
        sample(3, 30, 0.0, 2000),
        sample(4, 40, 0.0, 2000),
    ];
    let db = crate::usage::database(dir.path()).unwrap();
    let pending = resolve(&db, "test", "live", &samples[..3], None).unwrap();
    assert!(pending.windows[&300].candidate.is_some());
    drop(db);
    let db = crate::usage::database(dir.path()).unwrap();
    let live = resolve(&db, "test", "live", &samples, None).unwrap();
    assert_eq!(ended(&live)[0].notice_id.as_deref(), Some("3-300"));
    let mut historical = sample(-25, 25, 0.0, 2000);
    historical.value.observation_source = Some("history-log".into());
    let mut merged = samples.clone();
    merged.push(historical);
    let history = resolve(&db, "test", "history", &merged, Some(&live)).unwrap();
    assert_eq!(ended(&history)[0].notice_id.as_deref(), Some("3-300"));
    assert_eq!(history.events[0].after.at, 25_000);
    assert!(!history.events[0].historical);
    let again = resolve(&db, "test", "history", &merged, Some(&live)).unwrap();
    assert_eq!(ended(&again)[0].notice_id, ended(&history)[0].notice_id);
    assert!(again.windows[&300].candidate.is_none());
}

#[test]
fn reconsumption_long_gaps_and_unknown_accounts_do_not_invent_missing_events() {
    let samples = vec![
        sample(1, 10, 20.0, 1000),
        sample(2, 20, 20.0, 1000),
        sample(3, 30, 30.0, 2000),
        sample(4, 50, 31.0, 2000),
    ];
    let state = derive(&samples);
    assert!(ended(&state).is_empty());
    assert!(state.windows[&300].candidate.is_some());
    let mut unknown = vec![
        sample(1, 10, 80.0, 1000),
        sample(2, 20, 80.0, 1000),
        sample(3, 1_000_000, 0.0, 2_000_000),
        sample(4, 1_000_010, 0.0, 2_000_000),
    ];
    let state = derive(&unknown);
    assert_eq!(ended(&state).len(), 1);
    assert!(ended(&state)[0].incomplete);
    for row in &mut unknown {
        row.value.account_id = None;
    }
    let state = derive(&unknown);
    assert!(ended(&state).is_empty());
    assert!(state.windows[&300].cycle.incomplete);
}

#[test]
fn a_recovery_that_returns_to_the_old_deadline_cannot_confirm_the_anomalous_window() {
    let state = derive(&[
        sample(1, 10, 80.0, 1000),
        sample(2, 20, 80.0, 1000),
        sample(3, 30, 0.0, 2000),
        sample(4, 40, 1.0, 1000),
        sample(5, 50, 2.0, 1000),
    ]);
    assert!(ended(&state).is_empty());
    assert!(state.windows[&300].candidate.is_some());
}

#[test]
fn account_and_plan_boundaries_are_separate_from_early_reset_events() {
    let mut plan = sample(2, 20, 30.0, 1000);
    plan.value.plan_type = Some("pro".into());
    let mut returned = sample(3, 30, 30.0, 1000);
    returned.value.plan_type = Some("pro".into());
    returned.account_boundary = true;
    let state = derive(&[sample(1, 10, 30.0, 1000), plan, returned]);
    let events = ended(&state);
    assert_eq!(events.len(), 2);
    assert!(events.iter().all(|c| matches!(
        c.reset_type.as_deref(),
        Some("plan_changed" | "account_changed")
    )));
    assert_eq!(state.windows[&300].cycle.started_at, Some(30_000));
    assert!(state.windows[&300].cycle.incomplete);
}

#[test]
fn an_ambiguous_merge_never_uses_deadline_proximity_as_identity() {
    let samples = vec![
        sample(1, 10, 80.0, 1000),
        sample(2, 20, 80.0, 1000),
        sample(3, 30, 0.0, 2000),
        sample(4, 40, 0.0, 2000),
        sample(5, 50, 80.0, 2000),
        sample(6, 60, 0.0, 3000),
        sample(7, 70, 0.0, 3000),
    ];
    let old = derive(&samples);
    assert_eq!(old.events.len(), 2);
    let mut revised = derive(&samples[..4]);
    revised.events[0].members.extend([6, 7]);
    revised.reconcile(&old);
    assert_eq!(
        revised.events.len(),
        2,
        "two prior reset anchors in one revised episode are ambiguous: retain the prior periods"
    );
    assert!(revised.ambiguous);
}
#[test]
fn history_cannot_silently_erase_online_reset_anchors() {
    let live = derive(&[
        sample(1, 10, 80.0, 1000),
        sample(2, 20, 80.0, 1000),
        sample(3, 30, 0.0, 2000),
        sample(4, 40, 0.0, 2000),
    ]);
    let mut damaged = State::default();
    damaged.preserve_live(&live);
    assert_eq!(damaged.events.len(), live.events.len());
    assert!(damaged.ambiguous);
    assert!(damaged
        .warnings()
        .iter()
        .any(|w| w.contains("历史合并未能保留")));
}
#[test]
fn missing_provenance_is_not_evidence_that_newer_known_account_observations_are_stale() {
    let mut older = sample(1, 10, 80.0, 1000);
    older.value.observation_source = Some("history-log".into());
    let mut next = sample(2, 30, 0.0, 2000);
    next.value.observation_source = None;
    let mut confirmed = next.clone();
    confirmed.id = 3;
    confirmed.at = 40_000;
    confirmed.until = 40_000;
    assert_eq!(ended(&derive(&[older, next, confirmed])).len(), 1);
}
#[test]
fn a_deadline_candidate_must_not_freeze_consumption_and_hide_the_next_real_reset() {
    let state = derive(&[
        sample(1, 10, 0.0, 1000),
        sample(2, 20, 0.0, 1000),
        sample(3, 30, 0.0, 1020),
        sample(4, 40, 60.0, 1020),
        sample(5, 50, 100.0, 1020),
        sample(6, 60, 0.0, 2000),
        sample(7, 70, 0.0, 2000),
    ]);
    let rows = ended(&state);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].notice_id.as_deref(), Some("6-300"));
    assert_eq!(rows[0].remaining, 0.0);
    assert_eq!(state.windows[&300].cycle.started_at, Some(50_000));
}
