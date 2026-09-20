use rusqlite::{Connection, OpenFlags};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::{HashMap, HashSet},
    fs::{self, File},
    io::{BufRead, BufReader, Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
    time::SystemTime,
};

const PAGE_BYTES: u64 = 512 * 1024;

#[derive(Clone)]
struct Entry {
    chain: RolloutChain,
    page: PreviewPage,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct PreviewItem {
    kind: String,
    text: String,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
pub struct PreviewCursor {
    segment: usize,
    offset: u64,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewPage {
    items: Vec<PreviewItem>,
    before: Option<PreviewCursor>,
    current_user: Option<usize>,
}

#[derive(Clone, Debug, PartialEq)]
struct RolloutSegment {
    path: PathBuf,
    end: u64,
    len: u64,
    modified: Option<SystemTime>,
}

#[derive(Clone, Debug, PartialEq)]
struct RolloutChain {
    segments: Vec<RolloutSegment>,
}

impl RolloutChain {
    fn end_cursor(&self) -> PreviewCursor {
        PreviewCursor {
            segment: self.segments.len() - 1,
            offset: self.segments.last().unwrap().end,
        }
    }

    fn absolute_offset(&self, cursor: PreviewCursor) -> u64 {
        self.segments
            .iter()
            .take(cursor.segment)
            .map(|segment| segment.end)
            .sum::<u64>()
            + cursor.offset
    }

    fn at_start(&self, cursor: PreviewCursor) -> bool {
        cursor.segment == 0 && cursor.offset == 0
    }
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
}

pub fn read(id: &str, before: Option<PreviewCursor>) -> Result<PreviewPage, String> {
    let home = crate::source::codex_home()?;
    let db = Connection::open_with_flags(
        home.join("state_5.sqlite"),
        OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .map_err(|e| e.to_string())?;
    let path: String = db
        .query_row(
            "SELECT rollout_path FROM threads WHERE id=?1",
            [id],
            |row| row.get(0),
        )
        .map_err(|e| e.to_string())?;
    let path = PathBuf::from(path);
    let chain = rollout_chain(&home, id, &path)?;
    let end = before.unwrap_or_else(|| chain.end_cursor());
    let Some(segment) = chain.segments.get(end.segment) else {
        return Err("预览历史游标无效".into());
    };
    if end.offset > segment.end {
        return Err("预览历史游标无效".into());
    }
    let key = format!("{id}:{end:?}");
    static CACHE: OnceLock<Mutex<HashMap<String, Entry>>> = OnceLock::new();
    let mut cache = CACHE
        .get_or_init(Default::default)
        .lock()
        .map_err(|e| e.to_string())?;
    if let Some(entry) = cache.get(&key).filter(|entry| entry.chain == chain) {
        return Ok(entry.page.clone());
    }

    let mut size = PAGE_BYTES;
    let distance = chain.absolute_offset(end);
    let page = loop {
        let page = read_chain_page(&chain, end, size)?;
        if before.is_some()
            || page.current_user.is_some()
            || page.before.is_none()
            || size >= distance
        {
            break page;
        }
        let next_size = size.saturating_mul(2).min(distance);
        if next_size <= size {
            break page;
        }
        size = next_size;
    };
    if cache.len() >= 24 {
        cache.clear();
    }
    cache.insert(
        key,
        Entry {
            chain,
            page: page.clone(),
        },
    );
    Ok(page)
}

#[cfg(test)]
fn read_page(file: &mut File, end: u64, size: u64) -> Result<PreviewPage, String> {
    let start = end.saturating_sub(size);
    file.seek(SeekFrom::Start(start))
        .map_err(|e| e.to_string())?;
    let mut bytes = Vec::with_capacity((end - start) as usize);
    file.take(end - start)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    let skipped = if start == 0 {
        0
    } else {
        bytes
            .iter()
            .position(|byte| *byte == b'\n')
            .map_or(bytes.len(), |at| at + 1)
    };
    let actual_start = start + skipped as u64;
    if start > 0 && actual_start == end {
        let next_size = size.saturating_mul(2).min(end);
        if next_size > size {
            return read_page(file, end, next_size);
        }
    }
    let records = bytes[skipped..]
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
        .filter_map(|line| serde_json::from_slice::<Value>(line).ok())
        .collect::<Vec<_>>();
    if start > 0 && !has_user_context(&records) {
        let next_size = size.saturating_mul(2).min(end);
        if next_size > size {
            return read_page(file, end, next_size);
        }
    }
    let mut page = preview_items(&records);
    page.before = (actual_start > 0).then_some(PreviewCursor {
        segment: 0,
        offset: actual_start,
    });
    Ok(page)
}

fn rollout_chain(home: &Path, id: &str, current_path: &Path) -> Result<RolloutChain, String> {
    let current = rollout_descriptor(current_path)?;
    if current.meta.id != id {
        return Err("预览 rollout 与对话不匹配".into());
    }
    let mut descriptors = if current.meta.history_base.is_some() {
        rollout_descriptors(&home.join("sessions"), id)?
    } else {
        Vec::new()
    };
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
        });
        let Some(base) = current.meta.history_base.clone() else {
            break;
        };
        let parent = descriptors
            .iter()
            .filter(|candidate| candidate.path != current.path)
            .find(|candidate| {
                candidate.meta.id == id
                    && candidate.rollout_id == base.thread_id
                    && candidate.len >= base.end_byte_offset
                    && history_boundary_matches(candidate, &base)
            })
            .cloned()
            .ok_or_else(|| "无法定位预览 rollout 的历史文件".to_string())?;
        end = base.end_byte_offset;
        current = parent;
    }
    chain.reverse();
    Ok(RolloutChain { segments: chain })
}

fn rollout_descriptor(path: &Path) -> Result<RolloutDescriptor, String> {
    let file = File::open(path).map_err(|error| error.to_string())?;
    let mut line = String::new();
    BufReader::new(file)
        .read_line(&mut line)
        .map_err(|error| error.to_string())?;
    let record: RolloutMetaRecord = serde_json::from_str(&line)
        .map_err(|error| format!("无法解析预览 rollout 元数据：{error}"))?;
    let rollout_id = path
        .file_stem()
        .and_then(|name| name.to_str())
        .and_then(|name| name.rsplit_once('_').map(|(_, id)| id))
        .filter(|id| !id.is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| record.payload.id.clone());
    let metadata = fs::metadata(path).map_err(|error| error.to_string())?;
    Ok(RolloutDescriptor {
        path: path.to_path_buf(),
        meta: record.payload,
        rollout_id,
        len: metadata.len(),
        modified: metadata.modified().ok(),
    })
}

fn rollout_descriptors(root: &Path, id: &str) -> Result<Vec<RolloutDescriptor>, String> {
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
            } else if path
                .extension()
                .is_some_and(|extension| extension == "jsonl")
                && path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.contains(id))
            {
                if let Ok(descriptor) = rollout_descriptor(&path) {
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
    let Ok(mut file) = File::open(&candidate.path) else {
        return false;
    };
    if file
        .seek(SeekFrom::Start(base.end_byte_offset - 1))
        .ok()
        .and_then(|_| {
            let mut byte = [0u8; 1];
            file.read_exact(&mut byte).ok().map(|_| byte[0])
        })
        != Some(b'\n')
    {
        return false;
    }
    if file.seek(SeekFrom::Start(base.end_byte_offset)).is_err() {
        return false;
    }
    let mut line = String::new();
    if BufReader::new(file).read_line(&mut line).is_err() {
        return false;
    }
    serde_json::from_str::<Value>(&line)
        .ok()
        .and_then(|record| record["ordinal"].as_u64())
        .is_none_or(|ordinal| ordinal == base.end_ordinal_exclusive)
}

fn read_chain_page(
    chain: &RolloutChain,
    end: PreviewCursor,
    size: u64,
) -> Result<PreviewPage, String> {
    let (start, bytes) = read_chain_window(chain, end, size)?;
    let at_start = chain.at_start(start);
    let skipped = if at_start {
        0
    } else {
        bytes
            .iter()
            .position(|byte| *byte == b'\n')
            .map_or(bytes.len(), |at| at + 1)
    };
    let actual_start = advance_cursor(chain, start, skipped as u64);
    if !at_start && actual_start == end {
        let next_size = size.saturating_mul(2).min(chain.absolute_offset(end));
        if next_size > size {
            return read_chain_page(chain, end, next_size);
        }
    }
    let records = bytes[skipped..]
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
        .filter_map(|line| serde_json::from_slice::<Value>(line).ok())
        .collect::<Vec<_>>();
    if !at_start && !has_user_context(&records) {
        let next_size = size.saturating_mul(2).min(chain.absolute_offset(end));
        if next_size > size {
            return read_chain_page(chain, end, next_size);
        }
    }
    let mut page = preview_items(&records);
    page.before = (!chain.at_start(actual_start)).then_some(actual_start);
    Ok(page)
}

fn read_chain_window(
    chain: &RolloutChain,
    end: PreviewCursor,
    size: u64,
) -> Result<(PreviewCursor, Vec<u8>), String> {
    let Some(segment) = chain.segments.get(end.segment) else {
        return Err("预览历史游标无效".into());
    };
    let mut segment_index = end.segment;
    let mut segment_end = end.offset.min(segment.end);
    let mut remaining = size;
    let mut chunks = Vec::new();
    let start = loop {
        let take = segment_end.min(remaining);
        let segment_start = segment_end - take;
        if take > 0 {
            chunks.push(read_range(
                &chain.segments[segment_index].path,
                segment_start,
                segment_end,
            )?);
        }
        let start = PreviewCursor {
            segment: segment_index,
            offset: segment_start,
        };
        remaining -= take;
        if remaining == 0 || (segment_index == 0 && segment_start == 0) {
            break start;
        }
        segment_index -= 1;
        segment_end = chain.segments[segment_index].end;
    };
    chunks.reverse();
    let mut bytes = Vec::new();
    for chunk in chunks {
        bytes.extend(chunk);
    }
    Ok((start, bytes))
}

fn read_range(path: &Path, start: u64, end: u64) -> Result<Vec<u8>, String> {
    let mut file = File::open(path).map_err(|error| error.to_string())?;
    file.seek(SeekFrom::Start(start))
        .map_err(|error| error.to_string())?;
    let mut bytes = Vec::with_capacity((end - start) as usize);
    file.take(end - start)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    Ok(bytes)
}

fn advance_cursor(
    chain: &RolloutChain,
    mut cursor: PreviewCursor,
    mut bytes: u64,
) -> PreviewCursor {
    while bytes > 0 {
        let available = chain.segments[cursor.segment]
            .end
            .saturating_sub(cursor.offset);
        if bytes <= available {
            cursor.offset += bytes;
            return cursor;
        }
        bytes -= available;
        if cursor.segment + 1 >= chain.segments.len() {
            cursor.offset = chain.segments[cursor.segment].end;
            return cursor;
        }
        cursor.segment += 1;
        cursor.offset = 0;
    }
    cursor
}

fn message_text(payload: &Value) -> String {
    payload["content"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|part| part["text"].as_str())
        .filter(|text| !text.trim().is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

fn push(items: &mut Vec<PreviewItem>, kind: &str, text: impl Into<String>) {
    let text = text.into();
    if text.trim().is_empty()
        || items
            .last()
            .is_some_and(|item| item.kind == kind && item.text == text)
    {
        return;
    }
    items.push(PreviewItem {
        kind: kind.into(),
        text,
    });
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

fn system_label(text: &str) -> Option<String> {
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

fn push_user(items: &mut Vec<PreviewItem>, text: String, waiting_for_assistant: &mut bool) {
    if text.trim().is_empty()
        || items.last().is_some_and(|item| {
            matches!(item.kind.as_str(), "user" | "interruption") && item.text == text
        })
    {
        return;
    }
    if let Some(label) = system_label(&text) {
        push(items, "system", format!("{label}\n{text}"));
        return;
    }
    push(
        items,
        if *waiting_for_assistant {
            "interruption"
        } else {
            "user"
        },
        text,
    );
    *waiting_for_assistant = true;
}

fn payload_tool_name(payload: &Value) -> &str {
    payload["name"]
        .as_str()
        .or_else(|| payload["tool_name"].as_str())
        .unwrap_or_default()
}

fn is_compaction(record_type: &str, payload_type: &str, payload: &Value) -> bool {
    record_type == "compacted"
        || matches!(payload_type, "context_compaction" | "compaction")
        || payload_tool_name(payload)
            .to_ascii_lowercase()
            .contains("compact")
}

fn is_user_record(record: &Value) -> bool {
    let payload = &record["payload"];
    let record_type = record["type"].as_str().unwrap_or_default();
    let payload_type = payload["type"].as_str().unwrap_or_default();
    record_type == "event_msg" && payload_type == "user_message"
        || record_type == "response_item" && payload_type == "message" && payload["role"] == "user"
}

fn has_user_context(records: &[Value]) -> bool {
    let Some(first_user) = records.iter().position(is_user_record) else {
        return true;
    };
    records[..first_user].iter().any(|record| {
        record["type"] == "event_msg"
            && matches!(
                record["payload"]["type"].as_str(),
                Some("task_started" | "task_complete" | "turn_aborted")
            )
    })
}

fn preview_items(records: &[Value]) -> PreviewPage {
    let mut items = Vec::new();
    let mut waiting_for_assistant = false;
    for record in records {
        let payload = &record["payload"];
        let record_type = record["type"].as_str().unwrap_or_default();
        let payload_type = payload["type"].as_str().unwrap_or_default();
        if is_compaction(record_type, payload_type, payload) {
            push(&mut items, "compact", "Context automatically compacted");
            continue;
        }
        if record_type == "event_msg" && payload_type == "turn_aborted" {
            let detail = payload["reason"]
                .as_str()
                .unwrap_or("The previous turn was aborted.");
            push(&mut items, "aborted", format!("Turn aborted\n{detail}"));
            waiting_for_assistant = false;
            continue;
        }
        if record_type == "event_msg" && payload_type == "task_complete" {
            waiting_for_assistant = false;
            continue;
        }
        let response_message = record_type == "response_item" && payload_type == "message";
        if response_message && payload["role"] == "user" {
            push_user(
                &mut items,
                message_text(payload),
                &mut waiting_for_assistant,
            );
        } else if record_type == "event_msg" && payload_type == "user_message" {
            push_user(
                &mut items,
                payload["message"]
                    .as_str()
                    .or_else(|| payload["text"].as_str())
                    .unwrap_or_default()
                    .to_string(),
                &mut waiting_for_assistant,
            );
        } else if response_message && payload["role"] == "assistant" {
            push(&mut items, "assistant", message_text(payload));
        } else if record_type == "event_msg" && payload_type == "agent_message" {
            push(
                &mut items,
                "assistant",
                payload["message"].as_str().unwrap_or_default(),
            );
        } else if record_type == "response_item"
            && matches!(
                payload_type,
                "function_call" | "custom_tool_call" | "local_shell_call"
            )
        {
            push(&mut items, "process", "Used tools");
        }
    }
    let current_user = items
        .iter()
        .rposition(|item| matches!(item.kind.as_str(), "user" | "interruption"));
    PreviewPage {
        items,
        before: None,
        current_user,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::{
        fs::{self, OpenOptions},
        io::Write,
        time::{SystemTime, UNIX_EPOCH},
    };

    #[test]
    fn older_page_must_advance_past_oversized_record_boundary() {
        let path = std::env::temp_dir().join(format!(
            "codex-chat-pane-preview-{}-{}.jsonl",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("clock before epoch")
                .as_nanos()
        ));
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .read(true)
            .open(&path)
            .expect("create preview fixture");
        file.write_all(b"older\n")
            .and_then(|_| file.write_all(&[b'x'; 15]))
            .and_then(|_| file.write_all(b"\n"))
            .expect("write preview fixture");
        let end = file.metadata().expect("read fixture metadata").len();

        let page = read_page(&mut file, end, 8).expect("read preview page");

        assert!(page.before.is_none_or(|before| before.offset < end));
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn older_page_reads_past_record_without_fixed_cap() {
        let path = std::env::temp_dir().join(format!(
            "codex-chat-pane-preview-large-{}-{}.jsonl",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("clock before epoch")
                .as_nanos()
        ));
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .read(true)
            .open(&path)
            .expect("create large preview fixture");
        file.write_all(b"older\n")
            .and_then(|_| file.write_all(&[b'x'; (PAGE_BYTES * 16 + 1) as usize]))
            .and_then(|_| file.write_all(b"\n"))
            .expect("write large preview fixture");
        let end = file.metadata().expect("read fixture metadata").len();

        let page = read_page(&mut file, end, PAGE_BYTES).expect("read large preview page");

        assert!(
            page.before.is_none(),
            "the unbounded read must reach the file start"
        );
        let _ = std::fs::remove_file(path);
    }

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
        let latest = read_chain_page(
            &chain,
            chain.end_cursor(),
            child_path.metadata().unwrap().len(),
        )
        .unwrap();
        assert!(latest.items.iter().any(|item| item.text == "new"));
        let older = read_chain_page(&chain, latest.before.unwrap(), PAGE_BYTES).unwrap();
        assert!(older.items.iter().any(|item| item.text == "old"));
        assert!(older.items.iter().any(|item| item.text == "middle"));
        assert!(older.before.is_none());
    }

    #[test]
    fn older_page_keeps_turn_context_for_steer_classification() {
        let mut file = tempfile::tempfile().expect("create preview fixture");
        writeln!(file, "{}", json!({"type":"event_msg","payload":{"type":"task_started"}}))
            .and_then(|_| writeln!(file, "{}", json!({"type":"response_item","payload":{"type":"message","role":"user","content":[{"text":"question"}]}})))
            .and_then(|_| writeln!(file, "{}", json!({"type":"response_item","payload":{"type":"message","role":"assistant","content":[{"text":"x".repeat(2048)}]}})))
            .and_then(|_| writeln!(file, "{}", json!({"type":"response_item","payload":{"type":"message","role":"user","content":[{"text":"steer"}]}})))
            .and_then(|_| writeln!(file, "{}", json!({"type":"event_msg","payload":{"type":"task_complete"}})))
            .expect("write preview fixture");
        let end = file.metadata().expect("read fixture metadata").len();

        let page = read_page(&mut file, end, 256).expect("read preview page");
        let steer = page
            .items
            .iter()
            .find(|item| item.text == "steer")
            .expect("steer message");

        assert_eq!(steer.kind, "interruption");
    }

    #[test]
    fn preserves_visible_conversation_and_process_order() {
        let page = preview_items(&[
            json!({"type":"event_msg","payload":{"type":"user_message","message":"question"}}),
            json!({"type":"response_item","payload":{"type":"message","role":"user","content":[{"text":"question"}]}}),
            json!({"type":"response_item","payload":{"type":"message","role":"assistant","content":[{"text":"progress"}]}}),
            json!({"type":"response_item","payload":{"type":"function_call","name":"mcp__codegraph__explore"}}),
            json!({"type":"compacted","payload":{}}),
            json!({"type":"response_item","payload":{"type":"reasoning","content":[{"text":"private"}]}}),
            json!({"type":"event_msg","payload":{"type":"user_message","message":"latest"}}),
            json!({"type":"event_msg","payload":{"type":"user_message","message":"interrupted"}}),
            json!({"type":"event_msg","payload":{"type":"user_message","message":"# AGENTS.md instructions\n<INSTRUCTIONS>test</INSTRUCTIONS>"}}),
            json!({"type":"event_msg","payload":{"type":"agent_message","message":"answer"}}),
        ]);
        assert_eq!(page.current_user, Some(5));
        assert_eq!(page.items[0].kind, "user");
        assert_eq!(page.items[1].text, "progress");
        assert_eq!(page.items[2].kind, "process");
        assert_eq!(page.items[2].text, "Used tools");
        assert_eq!(page.items[3].kind, "compact");
        assert_eq!(page.items[3].text, "Context automatically compacted");
        assert_eq!(page.items[4].text, "latest");
        assert_eq!(page.items[5].kind, "interruption");
        assert_eq!(page.items[6].kind, "system");
        assert!(page.items[6].text.starts_with("AGENTS.md\n"));
        assert_eq!(page.items[7].text, "answer");
        assert!(!page.items.iter().any(|item| item.text == "private"));
    }

    #[test]
    fn labels_user_insertions_as_steer_until_turn_completion() {
        let page = preview_items(&[
            json!({"type":"event_msg","payload":{"type":"user_message","message":"question"}}),
            json!({"type":"event_msg","payload":{"type":"agent_message","message":"partial answer"}}),
            json!({"type":"event_msg","payload":{"type":"user_message","message":"steer"}}),
            json!({"type":"event_msg","payload":{"type":"task_complete"}}),
            json!({"type":"event_msg","payload":{"type":"user_message","message":"next question"}}),
        ]);
        let kinds: Vec<_> = page.items.iter().map(|item| item.kind.as_str()).collect();
        assert_eq!(kinds, ["user", "assistant", "interruption", "user"]);
    }

    #[test]
    fn merges_consecutive_tool_calls_without_swallowing_compaction() {
        let page = preview_items(&[
            json!({"type":"event_msg","payload":{"type":"user_message","message":"go"}}),
            json!({"type":"response_item","payload":{"type":"function_call","name":"inspect_local_file"}}),
            json!({"type":"response_item","payload":{"type":"function_call","name":"mcp__codegraph__explore"}}),
            json!({"type":"response_item","payload":{"type":"local_shell_call","command":"ls"}}),
            json!({"type":"response_item","payload":{"type":"function_call","name":"exec"}}),
            json!({"type":"compacted","payload":{}}),
            json!({"type":"response_item","payload":{"type":"function_call","name":"compact_context"}}),
            json!({"type":"response_item","payload":{"type":"function_call","name":"exec"}}),
            json!({"type":"event_msg","payload":{"type":"agent_message","message":"done"}}),
        ]);
        let texts: Vec<_> = page.items.iter().map(|item| item.text.as_str()).collect();
        assert_eq!(
            texts,
            [
                "go",
                "Used tools",
                "Context automatically compacted",
                "Used tools",
                "done"
            ]
        );
        assert_eq!(page.items[1].kind, "process");
        assert_eq!(page.items[2].kind, "compact");
        assert_eq!(page.items[3].kind, "process");
        assert!(!page
            .items
            .iter()
            .any(|item| item.text.starts_with("Used inspect")
                || item.text.contains("CodeGraph")
                || item.text == "Used exec"));
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
        let page = preview_items(&[
            json!({"type":"event_msg","payload":{"type":"turn_aborted","reason":"stopped"}}),
        ]);
        assert_eq!(page.items[0].kind, "aborted");
    }
}
