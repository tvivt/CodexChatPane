use super::Subagent;
use rusqlite::{Connection, OpenFlags};
use serde_json::Value;
use std::{collections::BTreeMap, io::BufRead, path::Path};

fn parent(value: &Value) -> Option<&str> {
    value["parent_thread_id"]
        .as_str()
        .or_else(|| value["subagent"]["thread_spawn"]["parent_thread_id"].as_str())
        .or_else(|| value["source"]["subagent"]["thread_spawn"]["parent_thread_id"].as_str())
}

fn metadata(path: &str, id: &str) -> Option<Value> {
    let log = crate::rollout_log::open(Path::new(path)).ok()?;
    let mut line = String::new();
    std::io::BufReader::new(log.reader_at(0).ok()?)
        .read_line(&mut line)
        .ok()?;
    let value: Value = serde_json::from_str(&line).ok()?;
    (value["type"] == "session_meta" && value["payload"]["id"] == id)
        .then(|| value["payload"].clone())
}

pub(super) fn direct(
    home: &Path,
    id: &str,
    observed: Vec<Subagent>,
) -> Result<Vec<Subagent>, String> {
    let mut observed: BTreeMap<_, _> = observed
        .into_iter()
        .map(|a| (a.thread_id.clone(), a))
        .collect();
    let db = Connection::open_with_flags(
        home.join("state_5.sqlite"),
        OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .map_err(|e| e.to_string())?;
    let has_source = db.prepare("SELECT source FROM threads LIMIT 0").is_ok();
    let query = if has_source {
        "SELECT id, source, model, rollout_path FROM threads ORDER BY id"
    } else {
        "SELECT id, NULL, model, rollout_path FROM threads ORDER BY id"
    };
    let mut statement = db.prepare(query).map_err(|e| e.to_string())?;
    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, String>(3)?,
            ))
        })
        .map_err(|e| e.to_string())?;
    let mut children = Vec::new();
    for row in rows {
        let (child, source, model, rollout) = row.map_err(|e| e.to_string())?;
        if child == id {
            continue;
        }
        let mut source = source
            .and_then(|s| serde_json::from_str::<Value>(&s).ok())
            .unwrap_or(Value::Null);
        // Older catalogs need only the observed candidate's header, never its full history.
        if parent(&source).is_none() && observed.contains_key(&child) {
            source = metadata(&rollout, &child).unwrap_or(Value::Null);
        }
        if parent(&source) != Some(id) {
            continue;
        }
        let path = source["subagent"]["thread_spawn"]["agent_path"]
            .as_str()
            .or_else(|| source["source"]["subagent"]["thread_spawn"]["agent_path"].as_str())
            .map(str::to_owned);
        let mut agent = observed.remove(&child).unwrap_or_else(|| Subagent {
            thread_id: child.clone(),
            name: path
                .as_deref()
                .unwrap_or(&child)
                .rsplit('/')
                .next()
                .unwrap_or(&child)
                .into(),
            path,
            model: None,
            status: "unknown".into(),
            summary: None,
            error: None,
            consumption: None,
        });
        agent.model = model;
        children.push(agent);
    }
    Ok(children)
}
