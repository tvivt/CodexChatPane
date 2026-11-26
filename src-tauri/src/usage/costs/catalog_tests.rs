use super::*;
use std::fs;

fn fixture() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let catalog = Connection::open(dir.path().join("state_5.sqlite")).unwrap();
    catalog.execute_batch("CREATE TABLE threads(id TEXT,title TEXT,name TEXT,created_at INTEGER,updated_at INTEGER,archived INTEGER,rollout_path TEXT,cwd TEXT,project_id TEXT,thread_source TEXT,source TEXT);
        INSERT INTO threads VALUES('chat','Prompt','Named',1,2,0,'','C:\\Project',NULL,'user','vscode');").unwrap();
    let log = dir.path().join("rollout.jsonl");
    fs::write(
        &log,
        "{\"type\":\"session_meta\",\"payload\":{\"id\":\"chat\"}}\n",
    )
    .unwrap();
    catalog
        .execute(
            "UPDATE threads SET rollout_path=?1",
            [log.to_str().unwrap()],
        )
        .unwrap();
    fs::write(dir.path().join(".codex-global-state.json"),r#"{"local-projects":{"p":{"name":"Project","rootPaths":["C:\\Project"]},"q":{"name":"Other","rootPaths":["C:\\Other"]}},"thread-project-assignments":{"chat":{"projectId":"p"}}}"#).unwrap();
    super::super::context::confirm(dir.path(), Some("metadata-test")).unwrap();
    let db = database(dir.path()).unwrap();
    let stamp = serde_json::to_string(&signature(dir.path(), "chat", &log).unwrap()).unwrap();
    db.execute(
        "INSERT INTO usage_threads VALUES('chat',?1,'Old name','known','Known project',NULL,NULL)",
        [stamp],
    )
    .unwrap();
    db.execute("INSERT INTO usage_requests VALUES('chat','request',1000,?1)", [r#"{"model":"gpt-5.4","tier":null,"input":100,"cached":90,"output":20,"reasoning":5,"writes":null}"#]).unwrap();
    dir
}

#[test]
fn index_without_catalog_preserves_known_project() {
    let dir = fixture();
    index(dir.path(), None, &Query::default(), |_, _| {}).unwrap();
    let db = database(dir.path()).unwrap();
    let project: (String, String) = db
        .query_row(
            "SELECT project_id,project FROM usage_threads WHERE id='chat'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(project, ("known".into(), "Known project".into()));
}

#[test]
fn invalid_catalog_cannot_erase_known_project() {
    let dir = fixture();
    let mut stale = crate::source::scan_catalog_at(dir.path()).unwrap();
    stale.state = "Stale".into();
    stale.error = Some("fixture unavailable".into());
    let foreign = fixture();
    let foreign = crate::source::scan_catalog_at(foreign.path()).unwrap();
    for snapshot in [&stale, &foreign] {
        index(dir.path(), Some(snapshot), &Query::default(), |_, _| {}).unwrap();
        sync_catalog(dir.path(), Some(snapshot)).unwrap();
        let db = database(dir.path()).unwrap();
        let project: String = db
            .query_row(
                "SELECT project_id FROM usage_threads WHERE id='chat'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(project, "known");
    }
}

#[test]
fn catalog_refresh_changes_groups_without_reindexing_requests() {
    let dir = fixture();
    let db = database(dir.path()).unwrap();
    let requests = || {
        db.prepare("SELECT thread_id,response_id,at,data FROM usage_requests ORDER BY thread_id,response_id").unwrap()
        .query_map([],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,i64>(2)?,r.get::<_,String>(3)?))).unwrap()
        .collect::<Result<Vec<_>,_>>().unwrap()
    };
    let days = || {
        db.prepare("SELECT day,revision FROM usage_days ORDER BY day")
            .unwrap()
            .query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap()
    };
    let before_requests = requests();
    let before_days = days();
    let before_signature: String = db
        .query_row(
            "SELECT signature FROM usage_threads WHERE id='chat'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    let query = Query {
        group: Some("project".into()),
        ..Default::default()
    };
    let before = report(dir.path(), query.clone()).unwrap();
    assert_eq!(before.rows[0].label, "Known project");
    let mut snapshot = crate::source::scan_catalog_at(dir.path()).unwrap();
    crate::rollout_log::read_stats(true);
    sync_catalog(dir.path(), Some(&snapshot)).unwrap();
    let after = report(dir.path(), query.clone()).unwrap();
    assert_eq!(after.rows[0].label, "Project");
    assert_eq!(
        serde_json::to_value(&before.totals).unwrap(),
        serde_json::to_value(&after.totals).unwrap()
    );
    assert_eq!(
        crate::rollout_log::read_stats(false).opens,
        0,
        "metadata refresh cannot read rollouts"
    );
    let state_path = dir.path().join(".codex-global-state.json");
    let mut state: serde_json::Value =
        serde_json::from_slice(&fs::read(&state_path).unwrap()).unwrap();
    state["projectless-thread-ids"] = serde_json::json!(["chat"]);
    fs::write(&state_path, state.to_string()).unwrap();
    snapshot = crate::source::scan_catalog_at(dir.path()).unwrap();
    sync_catalog(dir.path(), Some(&snapshot)).unwrap();
    assert_eq!(
        report(dir.path(), query.clone()).unwrap().rows[0].label,
        "无项目对话"
    );
    state["projectless-thread-ids"] = serde_json::json!([]);
    state["thread-project-assignments"] = serde_json::json!({"chat":{"projectId":"q"}});
    fs::write(&state_path, state.to_string()).unwrap();
    snapshot = crate::source::scan_catalog_at(dir.path()).unwrap();
    index(dir.path(), Some(&snapshot), &Query::default(), |_, _| {}).unwrap();
    let after = report(dir.path(), query).unwrap();
    assert_eq!(after.rows[0].label, "Other");
    assert_eq!(
        serde_json::to_value(&before.totals).unwrap(),
        serde_json::to_value(&after.totals).unwrap()
    );
    assert_eq!(requests(), before_requests);
    assert_eq!(days(), before_days);
    let after_signature: String = db
        .query_row(
            "SELECT signature FROM usage_threads WHERE id='chat'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(after_signature, before_signature);
}

#[test]
#[ignore = "updates app cache metadata only; requires a cache backup and the old Pane stopped"]
fn live_project_metadata_cache_refresh() {
    use std::hash::{Hash, Hasher};
    let home = crate::source::codex_home().unwrap();
    let backup =
        std::env::var("CODEX_PANE_METADATA_CACHE_BACKUP").expect("back up the app cache first");
    assert!(Path::new(&backup).is_file());
    let quota = Connection::open_with_flags(
        home.join(".codex-chat-pane/usage.sqlite"),
        OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap();
    let account: String = quota
        .query_row(
            "SELECT account FROM quota_samples ORDER BY id DESC LIMIT 1",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_ne!(account, "unknown");
    super::super::context::confirm(&home, Some(&account)).unwrap();
    let identity = super::super::context::identity(&home).unwrap();
    let hash = identity.bytes().fold(0xcbf29ce484222325u64, |h, b| {
        (h ^ u64::from(b)).wrapping_mul(0x100000001b3)
    });
    let path = home.join(format!(".codex-chat-pane/usage-cache-{hash:016x}.sqlite"));
    assert!(path.is_file(), "this check must use an existing cache");
    let original = Connection::open_with_flags(&path, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
    assert_eq!(
        original
            .pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
            .unwrap(),
        3
    );
    let saved = Connection::open_with_flags(backup, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
    let saved_identity: String = saved
        .query_row("SELECT value FROM cache_identity", [], |r| r.get(0))
        .unwrap();
    assert_eq!(
        saved_identity, identity,
        "backup must match the active cache"
    );
    let fingerprint = |db: &Connection| {
        let mut hash = std::collections::hash_map::DefaultHasher::new();
        let mut count = 0u64;
        let mut statement=db.prepare("SELECT thread_id,response_id,at,data FROM usage_requests ORDER BY thread_id,response_id").unwrap();
        for row in statement
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, Option<i64>>(2)?,
                    r.get::<_, String>(3)?,
                ))
            })
            .unwrap()
        {
            row.unwrap().hash(&mut hash);
            count += 1;
        }
        (count, hash.finish())
    };
    let before_rows = fingerprint(&original);
    assert_eq!(before_rows, fingerprint(&saved));
    let before = report(&home, Query::default()).unwrap().totals;
    let snapshot = crate::source::scan_catalog(None).unwrap();
    crate::rollout_log::read_stats(true);
    let started = std::time::Instant::now();
    sync_catalog(&home, Some(&snapshot)).unwrap();
    assert_eq!(crate::rollout_log::read_stats(false).opens, 0);
    let elapsed = started.elapsed();
    let db = database(&home).unwrap();
    for (id, expected) in [
        ("01a21617-c492-7091-98e8-d113a5870324", "CodexChatPane"),
        ("01a216c1-ab80-72c2-afc7-859ad2f3c279", "StockToolNew"),
    ] {
        let before: String = saved
            .query_row("SELECT project FROM usage_threads WHERE id=?1", [id], |r| {
                r.get(0)
            })
            .unwrap();
        let after: String = db
            .query_row("SELECT project FROM usage_threads WHERE id=?1", [id], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(after, expected);
        eprintln!("metadata usage {id}: {before} -> {after}");
    }
    let after = report(&home, Query::default()).unwrap().totals;
    assert_eq!(fingerprint(&db), before_rows);
    assert_eq!(before.input, after.input);
    assert_eq!(before.cached, after.cached);
    assert_eq!(before.output, after.output);
    assert_eq!(before.requests, after.requests);
    assert!((before.cost_usd - after.cost_usd).abs() < 1e-8);
    eprintln!("metadata usage sync: {elapsed:?}, requests={} input={} cached={} output={} USD before={:.12} after={:.12}; all request rows unchanged; zero rollout opens",after.requests,after.input,after.cached,after.output,before.cost_usd,after.cost_usd);
}
