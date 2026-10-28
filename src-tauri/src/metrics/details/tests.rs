use super::*;
use serde_json::json;
use std::io::Write;
fn fixture() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("main.jsonl");
    let db = rusqlite::Connection::open(dir.path().join("state_5.sqlite")).unwrap();
    db.execute_batch("CREATE TABLE threads(id TEXT PRIMARY KEY,title TEXT,cwd TEXT,model TEXT,archived INTEGER,created_at INTEGER,updated_at INTEGER,rollout_path TEXT);").unwrap();
    db.execute(
        "INSERT INTO threads VALUES('main','Main','','gpt-6.1-sol',0,1,2,?1)",
        [path.to_string_lossy().as_ref()],
    )
    .unwrap();
    let mut file = std::fs::File::create(path).unwrap();
    writeln!(
        file,
        "{}",
        json!({"type":"session_meta","payload":{"id":"main"}})
    )
    .unwrap();
    for index in 0..120 {
        for record in [
            json!({"type":"turn_context","payload":{"turn_id":format!("turn-{index}"),"model":"gpt-6.1-sol"}}),
            json!({"type":"event_msg","timestamp":format!("2026-10-28T01:{:02}:00Z",index%60),"payload":{"type":"task_started","turn_id":format!("turn-{index}")}}),
            json!({"type":"event_msg","payload":{"type":"item_completed","item":{"type":"Reasoning","id":format!("activity-{index}")},"started_at_ms":1793149200000i64+index*1000,"completed_at_ms":1793149200100i64+index*1000}}),
            json!({"type":"token_usage_record","payload":{"thread_id":"main","turn_id":format!("turn-{index}"),"response_id":format!("r-{index}"),"usage":{"input_tokens":100,"cached_input_tokens":90,"output_tokens":10,"reasoning_output_tokens":0}}}),
            json!({"type":"event_msg","timestamp":format!("2026-10-28T01:{:02}:01Z",index%60),"payload":{"type":"task_complete","turn_id":format!("turn-{index}")}}),
        ] {
            writeln!(file, "{record}").unwrap();
        }
    }
    dir
}
#[test]
fn compact_pages_retain_summary_counts_and_never_reparse_for_paging() {
    let dir = fixture();
    let home = dir.path();
    let full = super::super::read_at(home, "main", true).unwrap();
    let first = read(home, "main", 0, None).unwrap();
    assert_eq!(first.summary.request_count, full.summary.request_count);
    assert_eq!(
        first.summary.activity_count,
        full.turns
            .iter()
            .filter(|t| t.status == "completed")
            .map(|t| t.activities.len())
            .sum::<usize>()
    );
    assert_eq!(first.turn_total, Some(120));
    assert_eq!(first.turns.len(), 50);
    assert!(first
        .turns
        .iter()
        .all(|t| t.requests.is_empty() && t.activities.is_empty()));
    assert!(first.turns.iter().any(|t| t.metrics.activity_count > 0));
    crate::rollout_log::read_stats(true);
    let middle = read(home, "main", 50, first.detail_revision.as_deref()).unwrap();
    let last = read(home, "main", 100, first.detail_revision.as_deref()).unwrap();
    assert_eq!(middle.turns.len(), 50);
    assert_eq!(last.turns.len(), 20);
    assert_eq!(crate::rollout_log::read_stats(false).bytes, 0);
    assert_eq!(first.summary.total_tokens, last.summary.total_tokens);
    let ids: std::collections::HashSet<_> = first
        .turns
        .iter()
        .chain(&middle.turns)
        .chain(&last.turns)
        .map(|t| &t.turn_id)
        .collect();
    assert_eq!(ids.len(), 120);
    assert!(read(home, "main", 1, None).is_err());
    assert!(read(home, "main", 150, first.detail_revision.as_deref()).is_err());
    assert!(read(home, "main", 0, Some("invalid-revision")).is_err());
    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .open(home.join("main.jsonl"))
        .unwrap();
    writeln!(
        file,
        "{}",
        json!({"type":"turn_context","payload":{"turn_id":"new","model":"gpt-6.1-sol"}})
    )
    .unwrap();
    assert_eq!(
        read(home, "main", 50, first.detail_revision.as_deref())
            .unwrap()
            .detail_revision,
        first.detail_revision,
        "an active log cannot change a frozen pagination snapshot"
    );
    let reopened = read(home, "main", 0, None).unwrap();
    assert_ne!(reopened.detail_revision, first.detail_revision);
    crate::usage::context::confirm(home, Some("another")).unwrap();
    assert!(read(home, "main", 0, reopened.detail_revision.as_deref()).is_err());
}
#[test]
fn ten_thousand_requests_and_activities_are_not_in_the_view_payload() {
    let dir = fixture();
    let mut data = super::super::read_at(dir.path(), "main", true).unwrap();
    let turn = &mut data.turns[0];
    let request = turn.requests[0].clone();
    let activity = turn.activities[0].clone();
    turn.requests = vec![request; 10000];
    turn.activities = vec![activity; 10000];
    let data = project(data);
    assert_eq!(
        data.turns
            .iter()
            .find(|t| t.metrics.activity_count == 10000)
            .unwrap()
            .metrics
            .activity_count,
        10000
    );
    assert!(data
        .turns
        .iter()
        .all(|t| t.requests.capacity() == 0 && t.activities.capacity() == 0));
    let entry = Entry {
        signature: String::new(),
        context: String::new(),
        revision: "test".into(),
        data,
    };
    let bytes = serde_json::to_vec(&page(&entry, 0)).unwrap().len();
    assert!(bytes < 100000, "compact first page is bounded: {bytes}");
}
#[test]
#[ignore = "reads one selected real history and measures compact paging without changing source data"]
fn local_compact_details_sample() {
    let id = std::env::var("CODEX_PANE_VERIFY_METRICS_THREAD").unwrap();
    let home = crate::source::codex_home().unwrap();
    crate::rollout_log::read_stats(true);
    let start = std::time::Instant::now();
    let first = read(&home, &id, 0, None).unwrap();
    let cold = start.elapsed();
    let cold_bytes = crate::rollout_log::read_stats(true).bytes;
    let payload = serde_json::to_vec(&first).unwrap().len();
    let start = std::time::Instant::now();
    let next = read(
        &home,
        &id,
        if first.turn_total.unwrap() > 50 {
            50
        } else {
            0
        },
        first.detail_revision.as_deref(),
    )
    .unwrap();
    let warm = start.elapsed();
    assert_eq!(crate::rollout_log::read_stats(false).bytes, 0);
    assert!(first.turns.len() <= 50 && next.turns.len() <= 50);
    assert_eq!(first.summary.total_tokens, next.summary.total_tokens);
    assert!(first
        .turns
        .iter()
        .chain(&next.turns)
        .all(|t| t.requests.is_empty() && t.activities.is_empty()));
    println!("turns={} activities={} settled_requests={} cold_ms={} source_bytes={} page_ms={} page_source_bytes=0 first_payload_bytes={payload}",first.turn_total.unwrap(),first.summary.activity_count,first.summary.request_count,cold.as_millis(),cold_bytes,warm.as_millis());
}
