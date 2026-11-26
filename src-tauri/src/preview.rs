use crate::rollout_log;
pub(crate) mod transcript;
use rusqlite::{Connection, OpenFlags};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::HashSet,
    fs,
    io::{BufRead, BufReader, Read},
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
    time::SystemTime,
};

#[derive(Clone, Debug, PartialEq)]
struct RolloutSegment {
    path: PathBuf,
    end: u64,
    len: u64,
    modified: Option<SystemTime>,
    source: rollout_log::Stamp,
}

#[derive(Clone, Debug, PartialEq)]
struct RolloutChain {
    segments: Vec<RolloutSegment>,
}

#[derive(Clone, Deserialize)]
struct RolloutMetaRecord {
    payload: RolloutMeta,
}

#[derive(Clone, Deserialize)]
struct RolloutMeta {
    id: String,
    #[serde(default)]
    history_base: Option<HistoryBase>,
}

#[derive(Clone, Deserialize)]
struct HistoryBase {
    thread_id: String,
    end_byte_offset: u64,
    end_ordinal_exclusive: u64,
}

#[derive(Clone)]
struct RolloutDescriptor {
    path: PathBuf,
    meta: RolloutMeta,
    rollout_id: String,
    len: u64,
    modified: Option<SystemTime>,
    source: rollout_log::Stamp,
}

fn rollout_chain(home: &Path, id: &str, current_path: &Path) -> Result<RolloutChain, String> {
    rollout_chain_at(home, id, current_path, true)
}

fn rollout_chain_at(
    home: &Path,
    id: &str,
    current_path: &Path,
    lengths: bool,
) -> Result<RolloutChain, String> {
    type CachedChain = (PathBuf, String, PathBuf, bool, RolloutChain);
    static CHAINS: OnceLock<Mutex<Vec<CachedChain>>> = OnceLock::new();
    let cache = CHAINS.get_or_init(Default::default);
    let key = (
        home.to_path_buf(),
        id.to_owned(),
        rollout_log::logical_path(current_path),
        lengths,
    );
    let saved = cache
        .lock()
        .map_err(|e| e.to_string())?
        .iter()
        .find(|(h, i, p, l, _)| h == &key.0 && i == &key.1 && p == &key.2 && *l == lengths)
        .map(|entry| entry.4.clone());
    if let Some(chain) = saved.filter(|chain| {
        chain
            .segments
            .iter()
            .all(|s| rollout_log::open(&s.path).is_ok_and(|log| log.stamp == s.source))
    }) {
        let mut cache = cache.lock().map_err(|e| e.to_string())?;
        if let Some(index) = cache
            .iter()
            .position(|(h, i, p, l, _)| h == &key.0 && i == &key.1 && p == &key.2 && *l == lengths)
        {
            let entry = cache.remove(index);
            cache.push(entry);
        }
        return Ok(chain);
    }
    let current = rollout_descriptor_at(current_path, lengths)?;
    if current.meta.id != id {
        return Err("预览 rollout 与对话不匹配".into());
    }
    let mut descriptors = if current.meta.history_base.is_some() {
        rollout_descriptors_at(&home.join("sessions"), id, lengths)?
    } else {
        Vec::new()
    };
    if current.meta.history_base.is_some() {
        descriptors.extend(rollout_descriptors_at(
            &home.join("archived_sessions"),
            id,
            lengths,
        )?);
    }
    if !descriptors
        .iter()
        .any(|candidate| candidate.path == current.path)
    {
        descriptors.push(current.clone());
    }

    let mut chain = Vec::new();
    let mut current = current;
    let mut end = current.len;
    let mut visited = HashSet::new();
    loop {
        if !visited.insert(current.path.clone()) {
            return Err("预览 rollout 历史链存在循环".into());
        }
        chain.push(RolloutSegment {
            path: current.path.clone(),
            end: end.min(current.len),
            len: current.len,
            modified: current.modified,
            source: current.source.clone(),
        });
        let Some(base) = current.meta.history_base.clone() else {
            break;
        };
        if !descriptors
            .iter()
            .any(|candidate| candidate.rollout_id == base.thread_id)
        {
            descriptors.extend(rollout_descriptors_at(
                &home.join("sessions"),
                &base.thread_id,
                lengths,
            )?);
            descriptors.extend(rollout_descriptors_at(
                &home.join("archived_sessions"),
                &base.thread_id,
                lengths,
            )?);
        }
        let parent = descriptors
            .iter()
            .filter(|candidate| candidate.path != current.path)
            .find(|candidate| {
                candidate.rollout_id == base.thread_id
                    && candidate.len >= base.end_byte_offset
                    && history_boundary_matches(candidate, &base)
            })
            .cloned()
            .ok_or_else(|| "无法定位预览 rollout 的历史文件".to_string())?;
        end = base.end_byte_offset;
        current = parent;
    }
    chain.reverse();
    let chain = RolloutChain { segments: chain };
    let mut cache = cache.lock().map_err(|e| e.to_string())?;
    cache.retain(|(h, i, p, l, _)| !(h == &key.0 && i == &key.1 && p == &key.2 && *l == lengths));
    cache.push((key.0, key.1, key.2, lengths, chain.clone()));
    if cache.len() > 32 {
        cache.remove(0);
    }
    Ok(chain)
}

pub(crate) fn metrics_segments(
    home: &Path,
    id: &str,
    path: &Path,
) -> Result<Vec<(PathBuf, u64)>, String> {
    let path = match rollout_log::open(path) {
        Ok(_) => rollout_log::logical_path(path),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let mut candidates = rollout_descriptors_at(&home.join("sessions"), id, false)?;
            candidates.extend(rollout_descriptors_at(
                &home.join("archived_sessions"),
                id,
                false,
            )?);
            candidates
                .into_iter()
                .max_by_key(|entry| entry.modified)
                .ok_or_else(|| "无法定位对话日志".to_string())?
                .path
        }
        Err(error) => return Err(error.to_string()),
    };
    Ok(rollout_chain_at(home, id, &path, false)?
        .segments
        .into_iter()
        .map(|segment| (segment.path, segment.end))
        .collect())
}

fn rollout_descriptor_at(path: &Path, lengths: bool) -> Result<RolloutDescriptor, String> {
    let mut log = rollout_log::open(path).map_err(|error| error.to_string())?;
    let len = if log.compressed && !lengths {
        u64::MAX
    } else {
        log.len().map_err(|error| error.to_string())?
    };
    let source = log.stamp.clone();
    let mut line = String::new();
    BufReader::new(log.reader_at(0).map_err(|error| error.to_string())?)
        .read_line(&mut line)
        .map_err(|error| error.to_string())?;
    let record: RolloutMetaRecord = serde_json::from_str(&line)
        .map_err(|error| format!("无法解析预览 rollout 元数据：{error}"))?;
    let path = rollout_log::logical_path(path);
    let rollout_id = path
        .file_stem()
        .and_then(|name| name.to_str())
        .and_then(|name| name.rsplit_once('_').map(|(_, id)| id))
        .filter(|id| !id.is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| record.payload.id.clone());
    Ok(RolloutDescriptor {
        path,
        meta: record.payload,
        rollout_id,
        len,
        modified: source.modified,
        source,
    })
}

fn rollout_descriptors_at(
    root: &Path,
    id: &str,
    lengths: bool,
) -> Result<Vec<RolloutDescriptor>, String> {
    if !root.is_dir() {
        return Ok(Vec::new());
    }
    let mut directories = vec![root.to_path_buf()];
    let mut descriptors = Vec::new();
    while let Some(directory) = directories.pop() {
        for entry in fs::read_dir(directory).map_err(|error| error.to_string())? {
            let path = entry.map_err(|error| error.to_string())?.path();
            if path.is_dir() {
                directories.push(path);
            } else if rollout_log::is_log(&path)
                && path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.contains(id))
            {
                if descriptors
                    .iter()
                    .any(|entry: &RolloutDescriptor| entry.path == rollout_log::logical_path(&path))
                {
                    continue;
                }
                if let Ok(descriptor) = rollout_descriptor_at(&path, lengths) {
                    descriptors.push(descriptor);
                }
            }
        }
    }
    Ok(descriptors)
}

fn history_boundary_matches(candidate: &RolloutDescriptor, base: &HistoryBase) -> bool {
    if base.end_byte_offset == candidate.len {
        return true;
    }
    if base.end_byte_offset == 0 || base.end_byte_offset > candidate.len {
        return false;
    }
    let Ok(log) = rollout_log::open(&candidate.path) else {
        return false;
    };
    let Ok(reader) = log.reader_at(base.end_byte_offset - 1) else {
        return false;
    };
    let mut file = BufReader::new(reader);
    let mut byte = [0u8; 1];
    if file.read_exact(&mut byte).is_err() || byte[0] != b'\n' {
        return false;
    }
    let mut line = String::new();
    if file.read_line(&mut line).is_err() {
        return false;
    }
    serde_json::from_str::<Value>(&line)
        .ok()
        .and_then(|record| record["ordinal"].as_u64())
        .is_none_or(|ordinal| ordinal == base.end_ordinal_exclusive)
}

fn read_since(chain: &RolloutChain, mut skip: u64) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    for segment in &chain.segments {
        if skip >= segment.end {
            skip -= segment.end;
            continue;
        }
        let len = segment.end - skip;
        let file = rollout_log::open(&segment.path)
            .and_then(|log| log.reader_at(skip))
            .map_err(|e| e.to_string())?;
        let n = file
            .take(len)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        if n as u64 != len {
            return Err("预览日志在读取期间被截断，请重试".into());
        }
        skip = 0;
    }
    Ok(bytes)
}

pub(crate) fn message_text(payload: &Value) -> String {
    payload["content"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|part| part["text"].as_str())
        .filter(|text| !text.trim().is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

fn skill_label(text: &str) -> Option<String> {
    for marker in ["## Skill: ", "# Skill: ", "<skill name=\""] {
        if let Some(rest) = text.split(marker).nth(1) {
            let name = rest
                .split(['\n', '\r', '"', '>'])
                .next()
                .unwrap_or_default()
                .trim();
            if !name.is_empty() {
                return Some(format!("Skill: {name}"));
            }
        }
    }
    None
}

pub(crate) fn system_label(text: &str) -> Option<String> {
    let lower = text.to_ascii_lowercase();
    if lower.contains("<cited_memory")
        || lower.contains("<user_memory")
        || lower.contains("<memories")
        || lower.contains("<memory>")
        || lower.contains("## memories")
        || lower.contains("oai memory")
    {
        Some("Cited memory".into())
    } else if lower.contains("<turn_aborted>") {
        Some("Turn aborted".into())
    } else if text.contains("# AGENTS.md instructions") {
        Some("AGENTS.md".into())
    } else if text.contains("<recommended_plugins>") {
        Some("Recommended plugins".into())
    } else if let Some(label) = skill_label(text) {
        Some(label)
    } else if text.contains("<skills_instructions>") {
        Some("Skills".into())
    } else if text.contains("<environment_context>") {
        Some("Environment context".into())
    } else if text.contains("<app-context>") {
        Some("App context".into())
    } else if text.contains("<permissions instructions>") {
        Some("Permissions".into())
    } else if text.contains("<collaboration_mode>") {
        Some("Collaboration mode".into())
    } else if text.contains("<apps_instructions>") {
        Some("Apps".into())
    } else if text.contains("<plugins_instructions>") {
        Some("Plugins".into())
    } else if text.contains("<INSTRUCTIONS>") || text.contains("<developer") {
        Some("System context".into())
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn reads_chained_rollouts_across_history_base() {
        let directory = tempfile::tempdir().unwrap();
        let sessions = directory.path().join("sessions");
        fs::create_dir_all(&sessions).unwrap();
        let id = "01aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        let parent_id = "01bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
        let root_path = sessions.join(format!("rollout-{id}.jsonl"));
        let parent_path = sessions.join(format!("rollout-{id}_{parent_id}.jsonl"));
        let child_path = sessions.join(format!("rollout-{id}_child.jsonl"));
        let root_header = format!("{}\n", json!({"type":"session_meta","payload":{"id":id}}));
        let root_task = format!(
            "{}\n",
            json!({"ordinal":1,"type":"event_msg","payload":{"type":"task_started"}})
        );
        let root_user = format!(
            "{}\n",
            json!({"ordinal":2,"type":"event_msg","payload":{"type":"user_message","message":"old"}})
        );
        let root_anchor = format!(
            "{}\n",
            json!({"ordinal":3,"type":"event_msg","payload":{"type":"task_complete"}})
        );
        let base_offset = (root_header.len() + root_task.len() + root_user.len()) as u64;
        fs::write(
            &root_path,
            format!("{root_header}{root_task}{root_user}{root_anchor}"),
        )
        .unwrap();
        let parent_header = format!(
            "{}\n",
            json!({
                "type":"session_meta",
                "payload":{
                    "id":id,
                    "history_base":{
                        "thread_id":id,
                        "end_ordinal_exclusive":3,
                        "end_byte_offset":base_offset
                    }
                }
            })
        );
        let parent_task = format!(
            "{}\n",
            json!({"ordinal":3,"type":"event_msg","payload":{"type":"task_started"}})
        );
        let parent_user = format!(
            "{}\n",
            json!({"ordinal":4,"type":"event_msg","payload":{"type":"user_message","message":"middle"}})
        );
        let parent_anchor = format!(
            "{}\n",
            json!({"ordinal":5,"type":"event_msg","payload":{"type":"task_complete"}})
        );
        let parent_base_offset =
            (parent_header.len() + parent_task.len() + parent_user.len()) as u64;
        fs::write(
            &parent_path,
            format!("{parent_header}{parent_task}{parent_user}{parent_anchor}"),
        )
        .unwrap();
        let child_header = format!(
            "{}\n",
            json!({
                "type":"session_meta",
                "payload":{
                    "id":id,
                    "history_base":{
                        "thread_id":parent_id,
                        "end_ordinal_exclusive":5,
                        "end_byte_offset":parent_base_offset
                    }
                }
            })
        );
        let child_task = format!(
            "{}\n",
            json!({"ordinal":5,"type":"event_msg","payload":{"type":"task_started"}})
        );
        let child_user = format!(
            "{}\n",
            json!({"ordinal":6,"type":"event_msg","payload":{"type":"user_message","message":"new"}})
        );
        fs::write(
            &child_path,
            format!("{child_header}{child_task}{child_user}"),
        )
        .unwrap();

        let chain = rollout_chain(directory.path(), id, &child_path).unwrap();
        assert_eq!(chain.segments.len(), 3);
        assert_eq!(chain.segments[0].end, base_offset);
        assert_eq!(chain.segments[1].end, parent_base_offset);
        let full = transcript::chain_text(chain.clone());
        assert!(["old", "middle", "new"]
            .iter()
            .all(|s| full.iter().any(|t| t == s)));
        for path in [&root_path, &parent_path, &child_path] {
            crate::rollout_log::compress_fixture(path);
        }
        assert_eq!(
            transcript::chain_text(rollout_chain(directory.path(), id, &child_path).unwrap()),
            full
        );
    }

    #[test]
    fn preview_follows_compressed_history_from_another_thread() {
        let dir = tempfile::tempdir().unwrap();
        let sessions = dir.path().join("sessions");
        fs::create_dir_all(&sessions).unwrap();
        let parent = sessions.join("rollout-parent.jsonl");
        let child = sessions.join("rollout-child.jsonl");
        let base = format!(
            "{}\n{}\n",
            json!({"type":"session_meta","payload":{"id":"parent"}}),
            json!({"ordinal":1,"type":"event_msg","payload":{"type":"user_message","message":"inherited"}})
        );
        fs::write(
            &parent,
            format!(
                "{base}{}\n",
                json!({"ordinal":2,"type":"event_msg","payload":{"type":"task_complete"}})
            ),
        )
        .unwrap();
        fs::write(&child, format!("{}\n{}\n", json!({"type":"session_meta","payload":{"id":"child","history_base":{"thread_id":"parent","end_byte_offset":base.len(),"end_ordinal_exclusive":2}}}), json!({"ordinal":2,"type":"event_msg","payload":{"type":"user_message","message":"own"}}))).unwrap();
        crate::rollout_log::compress_fixture(&parent);
        crate::rollout_log::compress_fixture(&child);
        let chain = rollout_chain(dir.path(), "child", &child).unwrap();
        assert_eq!(chain.segments.len(), 2);
        let full = transcript::chain_text(chain);
        assert!(full.iter().any(|m| m == "inherited") && full.iter().any(|m| m == "own"));
    }

    #[test]
    #[ignore = "requires read-only access to local compressed Codex histories"]
    fn local_compressed_preview() {
        for id in [
            "01a0ff60-165e-7ff1-89c5-4d58b94ce669",
            "01a02fa9-33af-7bb2-a8b4-a35196a4c534",
            "01a04e37-f08c-7b73-9c9a-4ea0689a8f1e",
            "01a01dd5-dcd9-7de0-a49d-f3c98f13a908",
            "01a05176-d7cf-7a52-8f5d-bbf1ae68c2bc",
        ] {
            let update = transcript::read(id, None).unwrap();
            assert!(serde_json::to_value(update).unwrap()["items"]
                .as_array()
                .is_some_and(|v| !v.is_empty()));
        }
    }

    #[test]
    fn labels_system_context_without_ai_summarization() {
        assert_eq!(
            system_label("<recommended_plugins>...</recommended_plugins>"),
            Some("Recommended plugins".into())
        );
        assert_eq!(
            system_label("## Skill: frontend-design\nbody"),
            Some("Skill: frontend-design".into())
        );
        assert_eq!(
            system_label("<permissions instructions>body"),
            Some("Permissions".into())
        );
        assert_eq!(
            system_label("<user_memory>remember</user_memory>"),
            Some("Cited memory".into())
        );
        assert_eq!(
            system_label("<turn_aborted>stopped</turn_aborted>"),
            Some("Turn aborted".into())
        );
    }
}
