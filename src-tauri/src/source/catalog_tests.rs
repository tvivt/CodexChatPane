use super::*;
use serde_json::json;

#[test]
fn current_read_pin_order_and_explicit_membership() {
    let dir = tempfile::tempdir().unwrap();
    let database = dir.path().join("state_5.sqlite");
    let db = Connection::open(&database).unwrap();
    db.execute_batch("CREATE TABLE threads(id TEXT,title TEXT,name TEXT,created_at INTEGER,updated_at INTEGER,archived INTEGER,rollout_path TEXT,cwd TEXT,project_id TEXT,thread_source TEXT,source TEXT);
        INSERT INTO threads VALUES('a','A',NULL,1,2,0,'','\\\\?\\C:\\Second\\',NULL,'user','vscode');
        INSERT INTO threads VALUES('b','B',NULL,1,2,0,'','\\\\?\\C:\\Second',NULL,'user','vscode');
        INSERT INTO threads VALUES('c','C',NULL,1,2,0,'','\\\\?\\C:\\Second',NULL,'user','vscode');
        INSERT INTO threads VALUES('d','D',NULL,1,2,0,'','C:\\Other','p1','user','vscode');
        UPDATE threads SET project_id='p1' WHERE id='b';").unwrap();
    fs::write(
        dir.path().join(".codex-global-state.json"),
        json!({
            "local-projects": {
                "p1":{"name":"First","rootPaths":[r"C:\First",r"C:\Second"]},
                "p2":{"name":"Other","rootPaths":[r"C:\Other"]}
            },
            "pinned-thread-ids":["a"],"pinned-project-ids":["p1"],"project-order":["p1","p2"],
            "electron-thread-read-state-v1":{"unreadByIdentity":{"x":{"local":["a","b"]}}}
        })
        .to_string(),
    )
    .unwrap();
    let settings = Connection::open(dir.path().join(".codex-global-state.sqlite")).unwrap();
    settings
        .execute_batch(
            "CREATE TABLE settings(scope TEXT,key TEXT,value TEXT,PRIMARY KEY(scope,key));",
        )
        .unwrap();
    let set = |key: &str, value: serde_json::Value| {
        settings.execute("INSERT INTO settings VALUES('global',?1,?2) ON CONFLICT(scope,key) DO UPDATE SET value=excluded.value",[key,&value.to_string()]).unwrap();
    };
    set(
        "thread-project-assignments",
        json!({"b":{"projectId":"p2"},"c":{"projectId":"p1"}}),
    );
    set("projectless-thread-ids", json!(["c"]));
    set("pinned-thread-ids", json!([]));
    set("pinned-project-ids", json!([]));
    set("project-order", json!(["p2", "p1"]));
    let scan = || scan_catalog_at(dir.path());
    let snapshot = scan().unwrap();
    let chat = |s: &SourceSnapshot, id: &str| s.chats.iter().find(|c| c.id == id).unwrap().clone();
    assert_eq!(
        chat(&snapshot, "a").project_id,
        "synthetic:uncategorized",
        "even a matching secondary root cannot imply membership"
    );
    assert_eq!(
        chat(&snapshot, "b").project_id,
        "p2",
        "explicit assignment wins over every root"
    );
    assert_eq!(
        chat(&snapshot, "d").project_id,
        "p1",
        "a thread project_id is explicit even when cwd matches another project"
    );
    assert_eq!(
        chat(&snapshot, "c").project_id,
        "synthetic:uncategorized",
        "explicit no-project wins over assignment and roots"
    );
    assert_eq!(snapshot.projects[0].id, "p2");
    assert_eq!(
        snapshot.projects[1].path.as_deref(),
        Some(r"C:\First"),
        "the first root remains the folder-open target"
    );
    assert_eq!(
        chat(&snapshot, "a").codex_unread,
        Some(true),
        "missing SQLite key retains legacy read-state support"
    );
    set(
        "electron-thread-read-state-v1",
        json!({"unreadByIdentity":{"x":{"local":["b"],"remote":["a"]}}}),
    );
    let read = scan().unwrap();
    assert_eq!(chat(&read, "a").codex_unread, Some(false));
    assert_eq!(chat(&read, "b").codex_unread, Some(true));
    set(
        "electron-thread-read-state-v1",
        json!({"unreadByIdentity":{}}),
    );
    assert!(scan()
        .unwrap()
        .chats
        .iter()
        .all(|c| c.codex_unread == Some(false)));

    // Simulate successful desktop tools by changing only the isolated settings store,
    // then use the production read-back predicates used by the existing MCP actions.
    for enabled in [true, false] {
        set(
            "pinned-thread-ids",
            if enabled { json!(["a"]) } else { json!([]) },
        );
        set(
            "pinned-project-ids",
            if enabled { json!(["p1"]) } else { json!([]) },
        );
        let started = std::time::Instant::now();
        crate::codex_app_mcp::verify_action_with("a", "pin", None, Some(enabled), scan).unwrap();
        crate::codex_app_mcp::verify_project_pin_with("p1", enabled, scan).unwrap();
        assert!(
            started.elapsed() < std::time::Duration::from_secs(1),
            "read-back must not wait for the three-second timeout"
        );
        eprintln!(
            "metadata pin/unpin enabled={enabled}: {:?}",
            started.elapsed()
        );
        let snapshot = scan().unwrap();
        assert_eq!(chat(&snapshot, "a").codex_pinned, enabled);
        assert_eq!(
            snapshot
                .projects
                .iter()
                .find(|p| p.id == "p1")
                .unwrap()
                .codex_pinned,
            enabled
        );
    }
}
