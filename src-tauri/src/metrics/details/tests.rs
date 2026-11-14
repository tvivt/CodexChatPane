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
fn only_direct_children_are_listed_and_child_details_keep_normal_statistics() {
    let dir = fixture();
    let home = dir.path();
    let db = rusqlite::Connection::open(home.join("state_5.sqlite")).unwrap();
    db.execute_batch("ALTER TABLE threads ADD COLUMN source TEXT;")
        .unwrap();
    for (id, parent) in [("child", "main"), ("peer", "main"), ("grandchild", "child")] {
        let path = home.join(format!("{id}.jsonl"));
        let source = json!({"subagent":{"thread_spawn":{"parent_thread_id":parent,"agent_path":format!("/root/{id}")}}});
        db.execute("INSERT INTO threads(id,title,cwd,model,archived,created_at,updated_at,rollout_path,source) VALUES(?1,?1,'','gpt-6.1-sol',0,1,2,?2,?3)",
            rusqlite::params![id,path.to_string_lossy(),source.to_string()]).unwrap();
        let mut file = std::fs::File::create(path).unwrap();
        for value in [
            json!({"type":"session_meta","payload":{"id":id,"source":source,"thread_source":"subagent"}}),
            json!({"type":"turn_context","payload":{"thread_id":id,"turn_id":"own","model":"gpt-6.1-sol"}}),
            json!({"type":"event_msg","payload":{"type":"task_started","thread_id":id,"turn_id":"own"}}),
            json!({"type":"event_msg","payload":{"type":"item_completed","turn_id":"own","item":{"type":"AgentMessage","id":"answer"},"started_at_ms":1000,"completed_at_ms":2000}}),
            json!({"type":"token_usage_record","payload":{"thread_id":id,"turn_id":"own","response_id":format!("request-{id}"),"usage":{"input_tokens":100,"cached_input_tokens":90,"output_tokens":50,"reasoning_output_tokens":0}}}),
            json!({"type":"event_msg","payload":{"type":"item_completed","turn_id":"own","item":{"type":"SubAgentActivity","id":"parent-message","agent_thread_id":"main","agent_path":"/root","kind":"interacted"}}}),
            json!({"type":"event_msg","payload":{"type":"item_completed","turn_id":"own","item":{"type":"SubAgentActivity","id":"peer-message","agent_thread_id":"peer","agent_path":"/root/peer","kind":"interacted"}}}),
            json!({"type":"event_msg","payload":{"type":"task_complete","thread_id":id,"turn_id":"own"}}),
        ] {
            writeln!(file, "{value}").unwrap();
        }
    }
    let main = read(home, "main", 0, None).unwrap();
    assert_eq!(
        main.subagents
            .iter()
            .map(|a| a.thread_id.as_str())
            .collect::<Vec<_>>(),
        vec!["child", "peer"]
    );
    let child = read(home, "child", 0, None).unwrap();
    assert_eq!(
        child
            .subagents
            .iter()
            .map(|a| a.thread_id.as_str())
            .collect::<Vec<_>>(),
        vec!["grandchild"]
    );
    assert_eq!(child.summary.output_tokens, Some(50));
    assert_eq!(
        serde_json::to_value(&child.consumption).unwrap()["totals"]["output"],
        50
    );
    assert_eq!(child.turn_total, Some(1));
    assert!(read(home, "grandchild", 0, None)
        .unwrap()
        .subagents
        .is_empty());
    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .open(home.join("child.jsonl"))
        .unwrap();
    for value in [
        json!({"type":"turn_context","payload":{"thread_id":"child","turn_id":"interrupted","model":"gpt-6.1-sol"}}),
        json!({"type":"event_msg","payload":{"type":"task_started","thread_id":"child","turn_id":"interrupted"}}),
        json!({"type":"token_usage_record","payload":{"thread_id":"child","turn_id":"interrupted","response_id":"no-timing","usage":{"input_tokens":200,"cached_input_tokens":180,"output_tokens":70,"reasoning_output_tokens":0}}}),
        json!({"type":"event_msg","payload":{"type":"turn_aborted","thread_id":"child","turn_id":"interrupted"}}),
    ] {
        writeln!(file, "{value}").unwrap();
    }
    let summary = read_summary(home, "child").unwrap();
    assert_eq!(
        summary.summary.output_tokens,
        Some(120),
        "summary retains interrupted settled usage"
    );
    assert_eq!(summary.summary.turn_count, 2);
    assert_eq!(summary.summary.completed_turn_count, 1);
    assert_eq!(summary.summary.request_count, 2);
    assert_eq!(
        summary.summary.valid_request_count, 1,
        "missing timing affects speed only"
    );
    assert_eq!(
        serde_json::to_value(&summary.consumption).unwrap()["totals"]["output"],
        120
    );
    assert!(summary.subagents.is_empty(), "summary does not recurse");
    assert!(summary
        .turns
        .iter()
        .all(|t| t.requests.is_empty() && t.activities.is_empty()));
    assert_eq!(
        read(home, "main", 0, main.detail_revision.as_deref())
            .unwrap()
            .detail_revision,
        main.detail_revision,
        "summary loading retains the parent's frozen page"
    );
}

#[test]
#[ignore = "checks the two user-selected real child logs without editing source records"]
fn local_subagent_statistics_sample() {
    let home = crate::source::codex_home().unwrap();
    let parent = "01a0ff60-165e-7ff1-89c5-4d58b94ce669";
    let direct = super::super::agents::direct(&home, parent, vec![]).unwrap();
    for id in [
        "01a11b4b-f104-7e02-91eb-21bef797f8ab",
        "01a11b4c-484f-7ce1-875d-4bb629b49d50",
    ] {
        assert!(direct.iter().any(|a| a.thread_id == id));
        let start = std::time::Instant::now();
        let summary = read_summary(&home, id).unwrap();
        assert!(summary.summary.request_count > 0);
        assert!(summary.summary.output_tokens.is_some_and(|v| v > 0));
        let full = read(&home, id, 0, None).unwrap();
        assert!(full
            .subagents
            .iter()
            .all(|a| a.thread_id != parent && a.thread_id != id));
        assert!(!full.subagents.iter().any(|a| [
            "01a11b4b-f104-7e02-91eb-21bef797f8ab",
            "01a11b4c-484f-7ce1-875d-4bb629b49d50"
        ]
        .contains(&a.thread_id.as_str())));
        println!(
            "id={id} summary={} direct_children={} elapsed_ms={}",
            serde_json::to_string(&summary.summary).unwrap(),
            full.subagents.len(),
            start.elapsed().as_millis()
        );
    }
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
