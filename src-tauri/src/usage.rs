pub mod context;
pub mod costs;
pub mod ledger;
mod pricing;

use rusqlite::Connection;
use std::{
    path::Path,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

fn database(home: &Path) -> Result<Connection, String> {
    let directory = home.join(".codex-chat-pane");
    std::fs::create_dir_all(&directory).map_err(|e| e.to_string())?;
    open_database(&directory.join("usage.sqlite"))
}

fn open_database(path: &Path) -> Result<Connection, String> {
    let db = Connection::open(path).map_err(|e| e.to_string())?;
    db.busy_timeout(Duration::from_secs(3))
        .map_err(|e| e.to_string())?;
    db.execute_batch(
        "PRAGMA journal_mode=WAL;
         CREATE TABLE IF NOT EXISTS quota_samples(id INTEGER PRIMARY KEY,at INTEGER NOT NULL,until INTEGER NOT NULL,account TEXT NOT NULL,data TEXT NOT NULL);
         CREATE INDEX IF NOT EXISTS quota_account ON quota_samples(account,id);
         CREATE TABLE IF NOT EXISTS quota_ack(event_id TEXT PRIMARY KEY);
         CREATE TABLE IF NOT EXISTS usage_threads(id TEXT PRIMARY KEY,signature TEXT NOT NULL,title TEXT NOT NULL,project_id TEXT NOT NULL,project TEXT NOT NULL,parent TEXT,error TEXT);
         CREATE TABLE IF NOT EXISTS usage_requests(thread_id TEXT NOT NULL,response_id TEXT NOT NULL,at INTEGER,data TEXT NOT NULL,PRIMARY KEY(thread_id,response_id));
         CREATE INDEX IF NOT EXISTS usage_time ON usage_requests(at);"
    ).map_err(|e| e.to_string())?;
    Ok(db)
}
