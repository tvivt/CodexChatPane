use super::ChatMetrics;
use std::{
    path::Path,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex, OnceLock,
    },
};

pub(crate) const PAGE_SIZE: usize = 50;
struct Entry {
    signature: String,
    context: String,
    revision: String,
    data: ChatMetrics,
}
type Slot = Arc<Mutex<Option<Entry>>>;
type Slots = Vec<(std::path::PathBuf, String, Slot)>;
fn slots() -> &'static Mutex<Slots> {
    static CACHE: OnceLock<Mutex<Slots>> = OnceLock::new();
    CACHE.get_or_init(Default::default)
}
fn context(home: &Path) -> Result<String, String> {
    Ok(format!(
        "{:?}|{:?}|{:?}|{:?}",
        home.canonicalize().map_err(|e| e.to_string())?,
        crate::usage::context::identity(home).ok(),
        std::fs::metadata(home.join("auth.json"))
            .ok()
            .map(|m| (m.len(), m.modified().ok())),
        std::fs::metadata(home.join("state_5.sqlite"))
            .ok()
            .and_then(|m| m.created().ok())
    ))
}
fn slot(home: &Path, id: &str) -> Result<Slot, String> {
    let mut slots = slots().lock().map_err(|_| "详情缓存锁已损坏")?;
    if let Some(i) = slots.iter().position(|(h, old, _)| h == home && old == id) {
        let row = slots.remove(i);
        let result = row.2.clone();
        slots.push(row);
        return Ok(result);
    }
    let result = Arc::new(Mutex::new(None));
    slots.push((home.into(), id.into(), result.clone()));
    if slots.len() > 4 {
        slots.remove(0);
    }
    Ok(result)
}
pub(crate) fn strip(data: &mut ChatMetrics) {
    for turn in &mut data.turns {
        turn.metrics.activity_count = turn.activities.len();
        turn.requests = Vec::new();
        turn.activities = Vec::new();
    }
    data.usage_batches.clear();
}
fn project(mut data: ChatMetrics) -> ChatMetrics {
    strip(&mut data);
    let latest = data.latest_turn_id.clone();
    data.turns.sort_by_key(|t| {
        (
            std::cmp::Reverse(t.started_at),
            Some(&t.turn_id) != latest.as_ref(),
        )
    });
    data
}
fn page(entry: &Entry, offset: usize) -> ChatMetrics {
    // The cache contains summaries only; cloning a page never copies request/activity bodies.
    let source = &entry.data;
    ChatMetrics {
        thread_id: source.thread_id.clone(),
        title: source.title.clone(),
        cwd: source.cwd.clone(),
        model: source.model.clone(),
        effort: source.effort.clone(),
        archived: source.archived,
        created_at: source.created_at,
        updated_at: source.updated_at,
        as_of: source.as_of,
        complete: source.complete,
        warnings: source.warnings.clone(),
        latest_turn_id: source.latest_turn_id.clone(),
        summary: source.summary.clone(),
        turns: source
            .turns
            .iter()
            .skip(offset)
            .take(PAGE_SIZE)
            .cloned()
            .collect(),
        tools: source.tools.clone(),
        subagents: source.subagents.clone(),
        consumption: source.consumption.clone(),
        recent_speed: None,
        detail_revision: Some(entry.revision.clone()),
        turn_offset: Some(offset),
        turn_total: Some(source.turns.len()),
        parent_thread_id: source.parent_thread_id.clone(),
        usage_batches: vec![],
        usage_complete: source.usage_complete,
    }
}
pub(crate) fn read(
    home: &Path,
    id: &str,
    offset: usize,
    revision: Option<&str>,
) -> Result<ChatMetrics, String> {
    if offset % PAGE_SIZE != 0 {
        return Err("详情分页位置无效".into());
    }
    if revision
        .is_some_and(|v| v.is_empty() || v.len() > 32 || !v.bytes().all(|c| c.is_ascii_digit()))
    {
        return Err("详情快照 ID 无效".into());
    }
    let context = context(home)?;
    let slot = slot(home, id)?;
    let mut saved = slot.lock().map_err(|_| "详情缓存锁已损坏")?;
    if let Some(revision) = revision {
        let entry = saved
            .as_ref()
            .filter(|e| e.revision == revision && e.context == context)
            .ok_or("详情快照已失效，请重新打开详情")?;
        if offset >= entry.data.turns.len() && offset != 0 {
            return Err("详情分页超出范围".into());
        }
        return Ok(page(entry, offset));
    }
    let (path, header) = super::catalog(home, id)?;
    let signature = format!(
        "{context}|{:?}|{}|{:?}|{:?}|{}",
        crate::usage::costs::signature(home, id, Path::new(&path))?,
        header.title,
        header.model,
        header.updated_at,
        header.archived
    );
    if saved.as_ref().is_none_or(|e| e.signature != signature) {
        let mut data = super::read_at(home, id, true)?;
        if context != self::context(home)? {
            return Err("账号或日志来源已切换，请重新打开详情".into());
        }
        if data.usage_batches.is_empty() {
            data.usage_batches = data
                .turns
                .iter()
                .map(|t| (t.model.clone(), t.service_tier.clone(), t.requests.clone()))
                .collect();
        }
        if crate::usage::context::identity(home).is_ok() {
            let _ = crate::usage::costs::reuse(home, &data);
        }
        static REVISION: AtomicU64 = AtomicU64::new(1);
        *saved = Some(Entry {
            signature,
            context,
            revision: REVISION.fetch_add(1, Ordering::Relaxed).to_string(),
            data: project(data),
        });
    }
    let entry = saved.as_ref().unwrap();
    if offset >= entry.data.turns.len() && offset != 0 {
        return Err("详情分页超出范围".into());
    }
    Ok(page(entry, offset))
}

#[cfg(test)]
mod tests;
