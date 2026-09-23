use rusqlite::{Connection, OpenFlags};
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    env, fs,
    path::{Path, PathBuf},
};

mod diagnostics;
mod rollout;
use diagnostics::{Diagnostic, RateLimits};
use rollout::read_rollout_snapshot;

const REQUIRED_THREAD_COLUMNS: &[&str] = &[
    "id",
    "created_at",
    "updated_at",
    "title",
    "name",
    "archived",
    "rollout_path",
    "source",
    "thread_source",
    "cwd",
    "project_id",
];

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceSnapshot {
    pub state: String,
    pub error: Option<String>,
    pub projects: Vec<ProjectSnapshot>,
    pub chats: Vec<ChatSnapshot>,
    pub rate_limits: Option<RateLimits>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivityUpdate {
    pub chats: Vec<ChatSnapshot>,
    pub rate_limits: Option<RateLimits>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectSnapshot {
    pub id: String,
    pub name: String,
    pub path: Option<String>,
    pub error: Option<String>,
    pub count: usize,
    pub latest: i64,
    pub synthetic: bool,
    pub path_valid: bool,
    pub codex_pinned: bool,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatSnapshot {
    pub id: String,
    pub rollout_path: PathBuf,
    pub turn_id: Option<String>,
    pub project_id: String,
    pub title: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub archived: bool,
    pub codex_pinned: bool,
    pub codex_unread: Option<bool>,
    pub working: bool,
    pub last_user_message_at: i64,
    pub execution_started_at: Option<i64>,
    pub execution_ms: Option<i64>,
    pub execution_status: Option<String>,
    pub activity_at: i64,
    pub diagnostic: Option<Diagnostic>,
}

#[derive(Clone, Debug, Default)]
struct ThreadActivity {
    turn_id: Option<String>,
    working: bool,
    last_user_message_at: i64,
    execution_started_at: Option<i64>,
    execution_ms: Option<i64>,
    execution_status: Option<String>,
    activity_at: i64,
    diagnostic: Option<Diagnostic>,
    last_response_at: i64,
    observed: bool,
    waiting_for_tool: bool,
}

fn execution_duration(
    explicit: Option<i64>,
    started_at: Option<i64>,
    completed_at: Option<i64>,
) -> Option<i64> {
    explicit.filter(|duration| *duration >= 0).or_else(|| {
        completed_at?
            .checked_sub(started_at?)
            .filter(|duration| *duration >= 0)
    })
}

impl ThreadActivity {
    fn update_diagnostic(&mut self, retry: Option<&Diagnostic>, now: i64) {
        if self
            .diagnostic
            .as_ref()
            .is_some_and(|issue| issue.severity == "warning")
        {
            self.diagnostic = None;
        }
        let last_progress_at = self.last_response_at.max(self.last_user_message_at);
        if self.diagnostic.as_ref().is_some_and(|issue| {
            issue.kind == "noResponse" && last_progress_at > issue.at.saturating_sub(120_000)
        }) {
            self.diagnostic = None;
        }
        if !self.working {
            if self
                .diagnostic
                .as_ref()
                .is_some_and(|issue| issue.kind == "noResponse")
            {
                self.diagnostic = None;
            }
            return;
        }
        if self.waiting_for_tool {
            return;
        }
        if self.observed
            && self.last_user_message_at > 0
            && now.saturating_sub(last_progress_at) >= 120_000
        {
            self.diagnostic = Some(Diagnostic::no_response(last_progress_at));
        } else if self.diagnostic.is_none() {
            if let Some(retry) = retry.filter(|retry| retry.at > last_progress_at) {
                self.diagnostic = Some(retry.clone());
            }
        }
    }
}

#[derive(Debug)]
struct ProjectRow {
    id: String,
    name: String,
    path: Option<String>,
}

#[derive(Debug)]
struct ThreadRow {
    id: String,
    title: String,
    name: Option<String>,
    created_at: i64,
    updated_at: i64,
    archived: bool,
    rollout_path: PathBuf,
    cwd: String,
    project_id: Option<String>,
}

#[derive(Deserialize)]
struct GlobalState {
    #[serde(rename = "local-projects")]
    local_projects: HashMap<String, LocalProject>,
    #[serde(rename = "project-order", default)]
    project_order: Vec<String>,
    #[serde(rename = "thread-project-assignments", default)]
    thread_project_assignments: HashMap<String, ThreadProjectAssignment>,
    #[serde(rename = "pinned-thread-ids", default)]
    pinned_thread_ids: HashSet<String>,
    #[serde(rename = "pinned-project-ids", default)]
    pinned_project_ids: HashSet<String>,
    #[serde(rename = "projectless-thread-ids", default)]
    projectless_thread_ids: HashSet<String>,
    #[serde(rename = "electron-thread-read-state-v1", default)]
    thread_read_state: ThreadReadState,
}

#[derive(Default, Deserialize)]
struct ThreadReadState {
    #[serde(rename = "unreadByIdentity", default)]
    unread_by_identity: Option<HashMap<String, HashMap<String, HashSet<String>>>>,
}

impl ThreadReadState {
    fn local_unread(&self) -> Option<HashSet<String>> {
        let by_identity = self.unread_by_identity.as_ref()?;
        let mut unread = HashSet::new();
        for hosts in by_identity.values() {
            for (host, ids) in hosts {
                if host == "local" || host.starts_with("local:") {
                    unread.extend(ids.iter().cloned());
                }
            }
        }
        Some(unread)
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct LocalProject {
    name: String,
    #[serde(default)]
    root_paths: Vec<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ThreadProjectAssignment {
    project_id: String,
}

pub fn codex_home() -> Result<PathBuf, String> {
    if let Some(path) = env::var_os("CODEX_HOME").filter(|value| !value.is_empty()) {
        return Ok(PathBuf::from(path));
    }
    env::var_os("USERPROFILE")
        .map(PathBuf::from)
        .map(|path| path.join(".codex"))
        .ok_or_else(|| "无法确定 Codex home：CODEX_HOME 和 USERPROFILE 均未设置".to_string())
}

pub fn scan() -> Result<SourceSnapshot, String> {
    let home = codex_home()?;
    scan_path(&home.join("state_5.sqlite"), &home)
}

pub fn scan_catalog(previous: &SourceSnapshot) -> Result<SourceSnapshot, String> {
    let home = codex_home()?;
    scan_path_with_previous(&home.join("state_5.sqlite"), &home, Some(previous))
}

fn scan_path(database: &Path, home: &Path) -> Result<SourceSnapshot, String> {
    scan_path_with_previous(database, home, None)
}

fn scan_path_with_previous(
    database: &Path,
    home: &Path,
    previous: Option<&SourceSnapshot>,
) -> Result<SourceSnapshot, String> {
    if !database.is_file() {
        return Err(format!("Codex 状态库不存在：{}", database.display()));
    }

    let connection = Connection::open_with_flags(
        database,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|error| format!("无法只读打开 Codex 状态库：{error}"))?;

    require_table(&connection, "threads", REQUIRED_THREAD_COLUMNS)?;
    let (
        mut projects,
        assignments,
        projectless_thread_ids,
        pinned_thread_ids,
        pinned_project_ids,
        unread_thread_ids,
    ) = read_global_state(&home.join(".codex-global-state.json"))?;
    let threads = read_threads(&connection)?;
    let app_server_started_at = desktop_app_server_started_at();
    let mut thread_activity = if let Some(previous) = previous {
        previous
            .chats
            .iter()
            .map(|chat| {
                (
                    chat.id.clone(),
                    ThreadActivity {
                        turn_id: chat.turn_id.clone(),
                        working: chat.working,
                        last_user_message_at: chat.last_user_message_at,
                        execution_started_at: chat.execution_started_at,
                        execution_ms: chat.execution_ms,
                        execution_status: chat.execution_status.clone(),
                        activity_at: chat.activity_at,
                        diagnostic: chat.diagnostic.clone(),
                        ..Default::default()
                    },
                )
            })
            .collect()
    } else {
        read_thread_activity(&home.join("thread_history_1.sqlite"), app_server_started_at)
            .unwrap_or_default()
    };
    let mut rate_limits = previous.and_then(|snapshot| snapshot.rate_limits.clone());
    let previous_paths: HashMap<&str, &Path> = previous
        .map(|snapshot| {
            snapshot
                .chats
                .iter()
                .map(|chat| (chat.id.as_str(), chat.rollout_path.as_path()))
                .collect()
        })
        .unwrap_or_default();
    let history = previous.and_then(|_| {
        Connection::open_with_flags(
            home.join("thread_history_1.sqlite"),
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .ok()
    });
    let has_error = history.as_ref().is_some_and(|connection| {
        connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM pragma_table_info('thread_turns') WHERE name='error_json')",
                [],
                |row| row.get::<_, bool>(0),
            )
            .unwrap_or(false)
    });
    {
        for thread in &threads {
            if previous_paths.get(thread.id.as_str()) == Some(&thread.rollout_path.as_path()) {
                continue;
            }
            if let Some(activity) = history.as_ref().and_then(|connection| {
                read_thread_activity_for(connection, &thread.id, app_server_started_at, has_error)
                    .ok()
                    .flatten()
            }) {
                thread_activity.insert(thread.id.clone(), activity);
            }
            let Ok((rollout, quota)) =
                read_rollout_snapshot(&thread.rollout_path, app_server_started_at)
            else {
                continue;
            };
            if let Some(quota) = quota {
                if rate_limits
                    .as_ref()
                    .is_none_or(|old| quota.observed_at > old.observed_at)
                {
                    rate_limits = Some(quota);
                }
            }
            if let Some(activity) = rollout {
                merge_rollout_activity(&mut thread_activity, &thread.id, activity);
            }
        }
    }
    if previous.is_none() {
        let active_turns = thread_activity
            .values()
            .filter(|activity| activity.working)
            .filter_map(|activity| activity.turn_id.clone())
            .collect();
        let retries = env::var_os("LOCALAPPDATA")
            .map(|local| {
                diagnostics::read_retries(
                    &PathBuf::from(local).join("Codex/Logs"),
                    &active_turns,
                    app_server_started_at,
                )
            })
            .unwrap_or_default();
        let now = (time::OffsetDateTime::now_utc().unix_timestamp_nanos() / 1_000_000) as i64;
        for activity in thread_activity
            .values_mut()
            .filter(|activity| activity.working)
        {
            activity.update_diagnostic(
                activity.turn_id.as_ref().and_then(|turn| retries.get(turn)),
                now,
            );
        }
    }
    let known_project_ids: HashSet<String> = projects.iter().map(|row| row.id.clone()).collect();
    let project_ids_by_path: HashMap<String, String> = projects
        .iter()
        .filter_map(|project| {
            project
                .path
                .as_deref()
                .map(|path| (normalized_path(path), project.id.clone()))
        })
        .collect();
    let mut chats = Vec::with_capacity(threads.len());

    for thread in threads {
        let project_id = (!projectless_thread_ids.contains(&thread.id))
            .then(|| {
                assignments
                    .get(&thread.id)
                    .filter(|id| known_project_ids.contains(*id))
                    .cloned()
                    .or_else(|| {
                        thread
                            .project_id
                            .as_ref()
                            .filter(|id| known_project_ids.contains(*id))
                            .cloned()
                    })
                    .or_else(|| {
                        project_ids_by_path
                            .get(&normalized_path(&thread.cwd))
                            .cloned()
                    })
            })
            .flatten()
            .unwrap_or_else(|| "synthetic:uncategorized".to_string());
        let updated_at = seconds_or_millis(thread.updated_at);
        let codex_pinned = pinned_thread_ids.contains(&thread.id);
        let codex_unread = unread_thread_ids
            .as_ref()
            .map(|ids| ids.contains(&thread.id));
        let activity = thread_activity.get(&thread.id);

        chats.push(ChatSnapshot {
            id: thread.id,
            rollout_path: thread.rollout_path,
            turn_id: activity.and_then(|activity| activity.turn_id.clone()),
            project_id,
            title: thread
                .name
                .filter(|name| !name.trim().is_empty())
                .unwrap_or(thread.title),
            created_at: seconds_or_millis(thread.created_at),
            updated_at,
            archived: thread.archived,
            codex_pinned,
            codex_unread,
            working: activity.is_some_and(|activity| activity.working),
            last_user_message_at: activity
                .map(|activity| activity.last_user_message_at)
                .filter(|value| *value > 0)
                .unwrap_or(updated_at),
            execution_started_at: activity.and_then(|activity| activity.execution_started_at),
            execution_ms: activity.and_then(|activity| activity.execution_ms),
            execution_status: activity.and_then(|activity| activity.execution_status.clone()),
            activity_at: activity.map_or(updated_at, |activity| activity.activity_at),
            diagnostic: activity.and_then(|activity| activity.diagnostic.clone()),
        });
    }

    if chats
        .iter()
        .any(|chat| chat.project_id == "synthetic:uncategorized")
    {
        projects.push(ProjectRow {
            id: "synthetic:uncategorized".to_string(),
            name: "无项目对话".to_string(),
            path: None,
        });
    }

    let mut aggregates = HashMap::<String, (usize, i64)>::new();
    for chat in &chats {
        let aggregate = aggregates.entry(chat.project_id.clone()).or_default();
        aggregate.0 += 1;
        aggregate.1 = aggregate.1.max(chat.last_user_message_at);
    }

    let projects = projects
        .into_iter()
        .map(|project| {
            let (count, latest) = aggregates.get(&project.id).copied().unwrap_or_default();
            let codex_pinned = pinned_project_ids.contains(&project.id);
            let synthetic = project.id == "synthetic:uncategorized";
            let error = (!synthetic)
                .then(|| project_path_error(project.path.as_deref()))
                .flatten();
            ProjectSnapshot {
                synthetic,
                id: project.id,
                name: project.name,
                path: project.path,
                path_valid: error.is_none(),
                error,
                count,
                latest,
                codex_pinned,
            }
        })
        .collect();

    Ok(SourceSnapshot {
        state: "Ready".to_string(),
        error: None,
        projects,
        chats,
        rate_limits,
    })
}

fn merge_rollout_activity(
    activities: &mut HashMap<String, ThreadActivity>,
    id: &str,
    mut activity: ThreadActivity,
) {
    if let Some(current) = activities.get(id) {
        if current.turn_id.is_some() && current.turn_id == activity.turn_id {
            // Older task_complete records do not carry failure details.
            if current.execution_status.as_deref() == Some("failed")
                && activity.execution_status.as_deref() == Some("completed")
            {
                activity.execution_status = current.execution_status.clone();
            }
            activity.execution_ms = activity.execution_ms.or(current.execution_ms);
            activity.last_user_message_at = activity
                .last_user_message_at
                .max(current.last_user_message_at);
            activity.last_response_at = activity.last_response_at.max(current.last_response_at);
            if activity.diagnostic.is_none()
                && activity.execution_status.as_deref() == Some("failed")
            {
                activity.diagnostic = current.diagnostic.clone();
            }
        }
    }
    if activities
        .get(id)
        .is_none_or(|current| activity.activity_at >= current.activity_at)
    {
        activities.insert(id.to_string(), activity);
    }
}

pub fn refresh_activities(snapshot: &mut SourceSnapshot, ids: &[String]) -> ActivityUpdate {
    let home = codex_home().ok();
    let cutoff = desktop_app_server_started_at();
    let history = home.as_ref().and_then(|home| {
        Connection::open_with_flags(
            home.join("thread_history_1.sqlite"),
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .ok()
    });
    let has_error = history.as_ref().is_some_and(|connection| {
        connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM pragma_table_info('thread_turns') WHERE name='error_json')",
                [],
                |row| row.get::<_, bool>(0),
            )
            .unwrap_or(false)
    });
    let mut activities = HashMap::new();
    for id in ids {
        let Some(chat) = snapshot.chats.iter_mut().find(|chat| &chat.id == id) else {
            continue;
        };
        if let Some(activity) = history.as_ref().and_then(|connection| {
            read_thread_activity_for(connection, id, cutoff, has_error)
                .ok()
                .flatten()
        }) {
            activities.insert(id.clone(), activity);
        }
        if let Ok((activity, quota)) = read_rollout_snapshot(&chat.rollout_path, cutoff) {
            if let Some(activity) = activity {
                merge_rollout_activity(&mut activities, id, activity);
            }
            if let Some(quota) = quota {
                if snapshot
                    .rate_limits
                    .as_ref()
                    .is_none_or(|old| quota.observed_at > old.observed_at)
                {
                    snapshot.rate_limits = Some(quota);
                }
            }
        }
    }
    let active_turns = snapshot
        .chats
        .iter()
        .filter(|chat| chat.working)
        .filter_map(|chat| chat.turn_id.clone())
        .chain(
            activities
                .values()
                .filter(|activity| activity.working)
                .filter_map(|activity| activity.turn_id.clone()),
        )
        .collect();
    let retries = env::var_os("LOCALAPPDATA")
        .map(|local| {
            diagnostics::read_retries(
                &PathBuf::from(local).join("Codex/Logs"),
                &active_turns,
                cutoff,
            )
        })
        .unwrap_or_default();
    let now = (time::OffsetDateTime::now_utc().unix_timestamp_nanos() / 1_000_000) as i64;
    let mut changed = Vec::new();
    for chat in &mut snapshot.chats {
        if !ids.contains(&chat.id) {
            continue;
        }
        if let Some(mut activity) = activities.remove(&chat.id) {
            if activity.working {
                activity.update_diagnostic(
                    activity.turn_id.as_ref().and_then(|turn| retries.get(turn)),
                    now,
                );
            }
            chat.turn_id = activity.turn_id;
            chat.working = activity.working;
            chat.last_user_message_at = if activity.last_user_message_at > 0 {
                activity.last_user_message_at
            } else {
                chat.updated_at
            };
            chat.execution_started_at = activity.execution_started_at;
            chat.execution_ms = activity.execution_ms;
            chat.execution_status = activity.execution_status;
            chat.activity_at = activity.activity_at;
            chat.diagnostic = activity.diagnostic;
        } else if cutoff.is_none() {
            chat.working = false;
            chat.execution_started_at = None;
        }
        changed.push(chat.clone());
    }
    for chat in &changed {
        if let Some(project) = snapshot
            .projects
            .iter_mut()
            .find(|project| project.id == chat.project_id)
        {
            project.latest = project.latest.max(chat.last_user_message_at);
        }
    }
    ActivityUpdate {
        chats: changed,
        rate_limits: snapshot.rate_limits.clone(),
    }
}

fn read_thread_activity_for(
    connection: &Connection,
    id: &str,
    cutoff: Option<i64>,
    has_error: bool,
) -> Result<Option<ThreadActivity>, String> {
    let error_column = if has_error {
        "turns.error_json"
    } else {
        "NULL"
    };
    let sql = format!(
        "SELECT turns.thread_id, turns.status, turns.started_at, turns.completed_at, \
         turns.duration_ms, COALESCE((SELECT MAX(created_at_ms) FROM thread_items WHERE thread_id=?1 AND item_type='userMessage'), turns.started_at * 1000), \
         turns.turn_id, {error_column}, \
         (SELECT MAX(created_at_ms) FROM thread_items WHERE thread_id=?1 AND turn_id=turns.turn_id AND item_type='contextCompaction') \
         FROM thread_turns AS turns WHERE turns.thread_id=?1 \
         AND turns.first_user_item_id IS NOT NULL AND turns.started_at IS NOT NULL \
         ORDER BY turns.rollout_ordinal DESC LIMIT 1"
    );
    let mut statement = connection
        .prepare(&sql)
        .map_err(|error| error.to_string())?;
    let mut rows = statement.query([id]).map_err(|error| error.to_string())?;
    rows.next()
        .map_err(|error| error.to_string())?
        .map(|row| activity_from_row(row, cutoff).map(|(_, activity)| activity))
        .transpose()
        .map_err(|error| error.to_string())
}

fn read_thread_activity(
    database: &Path,
    app_server_started_at: Option<i64>,
) -> Result<HashMap<String, ThreadActivity>, String> {
    if !database.is_file() {
        return Ok(HashMap::new());
    }
    let connection = Connection::open_with_flags(
        database,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|error| format!("无法只读打开 Codex 历史库：{error}"))?;
    // Older history schemas still provide status, even without detailed errors.
    let has_error: bool = connection.query_row("SELECT EXISTS(SELECT 1 FROM pragma_table_info('thread_turns') WHERE name='error_json')", [], |row| row.get(0)).map_err(|error| error.to_string())?;
    let error_column = if has_error {
        "turns.error_json"
    } else {
        "NULL"
    };
    let mut statement = connection
        .prepare(
            &format!("WITH latest_turn AS (
               SELECT thread_id, MAX(rollout_ordinal) AS rollout_ordinal
               FROM thread_turns
               WHERE first_user_item_id IS NOT NULL AND started_at IS NOT NULL
               GROUP BY thread_id
             ), latest_message AS (
               SELECT thread_id, MAX(created_at_ms) AS created_at_ms
               FROM thread_items
               WHERE item_type = 'userMessage'
               GROUP BY thread_id
             )
             SELECT turns.thread_id, turns.status, turns.started_at, turns.completed_at,
                    turns.duration_ms, COALESCE(messages.created_at_ms, turns.started_at * 1000), turns.turn_id, {error_column},
                    (SELECT MAX(created_at_ms) FROM thread_items WHERE thread_id=turns.thread_id AND turn_id=turns.turn_id AND item_type='contextCompaction')
             FROM latest_turn
             JOIN thread_turns AS turns
               ON turns.thread_id = latest_turn.thread_id
              AND turns.rollout_ordinal = latest_turn.rollout_ordinal
             LEFT JOIN latest_message AS messages ON messages.thread_id = turns.thread_id"),
        )
        .map_err(|error| format!("无法读取 Codex 活动状态：{error}"))?;
    let rows = statement
        .query_map([], |row| activity_from_row(row, app_server_started_at))
        .map_err(|error| format!("无法查询 Codex 活动状态：{error}"))?;
    rows.collect::<Result<HashMap<_, _>, _>>()
        .map_err(|error| format!("无法解码 Codex 活动状态：{error}"))
}

fn activity_from_row(
    row: &rusqlite::Row<'_>,
    app_server_started_at: Option<i64>,
) -> rusqlite::Result<(String, ThreadActivity)> {
    let thread_id: String = row.get(0)?;
    let status: String = row.get(1)?;
    let started_at: i64 = row.get(2)?;
    let completed_at: Option<i64> = row.get(3)?;
    let duration_ms: Option<i64> = row.get(4)?;
    let duration_ms = execution_duration(
        duration_ms,
        Some(seconds_or_millis(started_at)),
        completed_at.map(seconds_or_millis),
    );
    let error_json: Option<String> = row.get(7)?;
    let diagnostic = error_json
        .and_then(|value| serde_json::from_str::<serde_json::Value>(&value).ok())
        .filter(|value| !value.is_null())
        .map(|value| {
            Diagnostic::from_error(
                &value,
                seconds_or_millis(completed_at.unwrap_or(started_at)),
                "thread-history",
            )
        });
    let working =
        status == "inProgress" && app_server_started_at.is_some_and(|cutoff| started_at >= cutoff);
    Ok((
        thread_id,
        ThreadActivity {
            turn_id: row.get(6)?,
            working,
            last_user_message_at: row.get(5)?,
            execution_started_at: working.then(|| seconds_or_millis(started_at)),
            execution_ms: (!working && status != "interrupted")
                .then_some(duration_ms)
                .flatten(),
            execution_status: (!(status == "inProgress" && !working)).then_some(status),
            activity_at: seconds_or_millis(completed_at.unwrap_or(started_at)),
            diagnostic,
            last_response_at: row.get::<_, Option<i64>>(8)?.unwrap_or(0),
            ..Default::default()
        },
    ))
}

#[cfg(windows)]
fn desktop_app_server_started_at() -> Option<i64> {
    use std::ffi::c_void;

    type Handle = *mut c_void;
    const INVALID_HANDLE_VALUE: Handle = -1_isize as Handle;
    const TH32CS_SNAPPROCESS: u32 = 2;
    const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;

    #[repr(C)]
    #[derive(Default)]
    struct FileTime {
        low: u32,
        high: u32,
    }

    #[repr(C)]
    struct ProcessEntry {
        size: u32,
        usage: u32,
        process_id: u32,
        default_heap_id: usize,
        module_id: u32,
        threads: u32,
        parent_process_id: u32,
        priority: i32,
        flags: u32,
        exe_file: [u16; 260],
    }

    impl Default for ProcessEntry {
        fn default() -> Self {
            unsafe { std::mem::zeroed() }
        }
    }

    #[link(name = "kernel32")]
    extern "system" {
        fn CreateToolhelp32Snapshot(flags: u32, process_id: u32) -> Handle;
        fn Process32FirstW(snapshot: Handle, entry: *mut ProcessEntry) -> i32;
        fn Process32NextW(snapshot: Handle, entry: *mut ProcessEntry) -> i32;
        fn OpenProcess(access: u32, inherit_handle: i32, process_id: u32) -> Handle;
        fn GetProcessTimes(
            process: Handle,
            creation: *mut FileTime,
            exit: *mut FileTime,
            kernel: *mut FileTime,
            user: *mut FileTime,
        ) -> i32;
        fn CloseHandle(handle: Handle) -> i32;
    }

    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snapshot == INVALID_HANDLE_VALUE {
            return None;
        }
        let mut entry = ProcessEntry::default();
        entry.size = std::mem::size_of::<ProcessEntry>() as u32;
        let mut processes = Vec::new();
        let mut has_entry = Process32FirstW(snapshot, &mut entry) != 0;
        while has_entry {
            let end = entry
                .exe_file
                .iter()
                .position(|value| *value == 0)
                .unwrap_or(entry.exe_file.len());
            processes.push((
                entry.process_id,
                entry.parent_process_id,
                String::from_utf16_lossy(&entry.exe_file[..end]),
            ));
            has_entry = Process32NextW(snapshot, &mut entry) != 0;
        }
        CloseHandle(snapshot);

        processes
            .iter()
            .filter(|(_, parent_id, name)| {
                name.eq_ignore_ascii_case("codex.exe")
                    && processes.iter().any(|(id, _, parent_name)| {
                        id == parent_id && parent_name.eq_ignore_ascii_case("ChatGPT.exe")
                    })
            })
            .filter_map(|(process_id, _, _)| {
                let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, *process_id);
                if process.is_null() {
                    return None;
                }
                let mut creation = FileTime::default();
                let mut exit = FileTime::default();
                let mut kernel = FileTime::default();
                let mut user = FileTime::default();
                let succeeded =
                    GetProcessTimes(process, &mut creation, &mut exit, &mut kernel, &mut user) != 0;
                CloseHandle(process);
                succeeded.then(|| {
                    let ticks = ((creation.high as u64) << 32) | creation.low as u64;
                    (ticks / 10_000_000) as i64 - 11_644_473_600
                })
            })
            .min()
    }
}

#[cfg(not(windows))]
fn desktop_app_server_started_at() -> Option<i64> {
    None
}

fn read_global_state(
    path: &Path,
) -> Result<
    (
        Vec<ProjectRow>,
        HashMap<String, String>,
        HashSet<String>,
        HashSet<String>,
        HashSet<String>,
        Option<HashSet<String>>,
    ),
    String,
> {
    let bytes = fs::read(path).map_err(|error| format!("无法读取 Codex 全局状态：{error}"))?;
    let state: GlobalState = serde_json::from_slice(&bytes)
        .map_err(|error| format!("无法解析 Codex 全局状态：{error}"))?;
    let order: HashMap<_, _> = state
        .project_order
        .into_iter()
        .enumerate()
        .map(|(index, id)| (id, index))
        .collect();
    let mut projects: Vec<_> = state.local_projects.into_iter().collect();
    projects.sort_by(|(left_id, left), (right_id, right)| {
        order
            .get(left_id)
            .copied()
            .unwrap_or(usize::MAX)
            .cmp(&order.get(right_id).copied().unwrap_or(usize::MAX))
            .then_with(|| left.name.cmp(&right.name))
            .then_with(|| left_id.cmp(right_id))
    });
    let projects = projects
        .into_iter()
        .map(|(id, project)| ProjectRow {
            id,
            name: project.name,
            path: project.root_paths.into_iter().next(),
        })
        .collect();
    let assignments = state
        .thread_project_assignments
        .into_iter()
        .map(|(thread_id, assignment)| (thread_id, assignment.project_id))
        .collect();
    let unread_thread_ids = state.thread_read_state.local_unread();
    Ok((
        projects,
        assignments,
        state.projectless_thread_ids,
        state.pinned_thread_ids,
        state.pinned_project_ids,
        unread_thread_ids,
    ))
}

fn read_threads(connection: &Connection) -> Result<Vec<ThreadRow>, String> {
    let mut statement = connection
        .prepare(
            "SELECT id, title, name, created_at, updated_at, archived, rollout_path, cwd, project_id\n             FROM threads\n             WHERE COALESCE(thread_source, '') NOT IN ('guardian_review', 'subagent')\n             ORDER BY updated_at DESC, id",
        )
        .map_err(|error| format!("无法读取 Codex Thread schema：{error}"))?;
    let rows = statement
        .query_map([], |row| {
            Ok(ThreadRow {
                id: row.get(0)?,
                title: row.get(1)?,
                name: row.get(2)?,
                created_at: row.get(3)?,
                updated_at: row.get(4)?,
                archived: row.get::<_, i64>(5)? != 0,
                rollout_path: PathBuf::from(row.get::<_, String>(6)?),
                cwd: row.get(7)?,
                project_id: row.get(8)?,
            })
        })
        .map_err(|error| format!("无法查询 Codex Chat：{error}"))?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("无法解码 Codex Chat：{error}"))
}

fn normalized_path(path: &str) -> String {
    path.trim_end_matches(|character| character == '/' || character == '\\')
        .replace('/', "\\")
        .to_lowercase()
}

fn project_path_error(path: Option<&str>) -> Option<String> {
    let Some(path) = path.filter(|path| !path.trim().is_empty()) else {
        return Some("未配置项目文件夹".to_string());
    };
    match fs::metadata(path) {
        Ok(metadata) if metadata.is_dir() => None,
        Ok(_) => Some(format!("项目路径不是文件夹：{path}")),
        Err(error) => Some(format!("无法访问项目路径：{path}（{error}）")),
    }
}

fn require_table(connection: &Connection, table: &str, required: &[&str]) -> Result<(), String> {
    let mut statement = connection
        .prepare(&format!("PRAGMA table_info({table})"))
        .map_err(|error| format!("无法检查 {table} schema：{error}"))?;
    let columns = statement
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(|error| format!("无法读取 {table} schema：{error}"))?
        .collect::<Result<HashSet<_>, _>>()
        .map_err(|error| format!("无法解码 {table} schema：{error}"))?;
    let missing: Vec<_> = required
        .iter()
        .filter(|column| !columns.contains(**column))
        .copied()
        .collect();
    if missing.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "Unsupported Codex schema：{table} 缺少 {}",
            missing.join(", ")
        ))
    }
}

fn seconds_or_millis(value: i64) -> i64 {
    if value > 10_000_000_000 {
        value
    } else {
        value.saturating_mul(1000)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn explains_project_path_errors() {
        let directory = tempdir().unwrap();
        assert!(project_path_error(directory.path().to_str()).is_none());
        let file = directory.path().join("not-a-folder");
        fs::write(&file, "content").unwrap();
        assert!(project_path_error(file.to_str())
            .unwrap()
            .contains("不是文件夹"));
        let missing = directory.path().join("missing");
        assert!(project_path_error(missing.to_str())
            .unwrap()
            .contains("无法访问项目路径"));
        assert_eq!(
            project_path_error(None).as_deref(),
            Some("未配置项目文件夹")
        );
    }

    #[test]
    fn uses_global_assignments_and_message_update_time() {
        let directory = tempdir().unwrap();
        let database = directory.path().join("state_5.sqlite");
        let connection = Connection::open(&database).unwrap();
        connection.execute_batch(
            "CREATE TABLE threads(\n               id TEXT PRIMARY KEY, title TEXT NOT NULL, name TEXT, created_at INTEGER NOT NULL,\n               updated_at INTEGER NOT NULL, recency_at INTEGER NOT NULL, archived INTEGER NOT NULL, rollout_path TEXT NOT NULL,\n               source TEXT NOT NULL, thread_source TEXT, cwd TEXT NOT NULL, project_id TEXT\n             );\n             INSERT INTO threads VALUES('t1', 'fallback', 'Visible', 1, 2, 99, 0, '', 'vscode', 'user', 'C:\\Pane', NULL);\n             INSERT INTO threads VALUES('t2', 'Loose', NULL, 1, 1, 100, 0, '', 'vscode', 'user', 'C:\\Pane\\child', NULL);\n             INSERT INTO threads VALUES('t3', 'Exact path', NULL, 1, 4, 102, 0, '', 'vscode', 'user', 'C:\\Empty', NULL);\n             INSERT INTO threads VALUES('guardian', 'internal', NULL, 1, 3, 101, 0, '', '{\"subagent\":{\"other\":\"guardian\"}}', 'subagent', 'C:\\Pane', NULL);\n             INSERT INTO threads VALUES('worker', 'child agent', NULL, 1, 5, 103, 0, '', 'vscode', 'subagent', 'C:\\Pane', NULL);",
        ).unwrap();
        drop(connection);
        fs::write(
            directory.path().join(".codex-global-state.json"),
            r#"{
              "local-projects": {
                "p1": { "name": "Pane", "rootPaths": ["C:\\Pane"] },
                "p2": { "name": "Empty", "rootPaths": ["C:\\Empty"] }
              },
              "project-order": ["p1", "p2"],
              "thread-project-assignments": {
                "t1": { "projectId": "p1" },
                "t2": { "projectId": "p1" }
              },
              "projectless-thread-ids": ["t2"],
              "pinned-thread-ids": ["t1"],
              "electron-thread-read-state-v1": {
                "unreadByIdentity": {
                  "identity-a": { "local:account-a": ["t1"], "remote:account-a": ["t2"] }
                }
              }
            }"#,
        )
        .unwrap();

        let snapshot = scan_path(&database, directory.path()).unwrap();
        assert_eq!(snapshot.projects.len(), 3);
        assert_eq!(snapshot.projects[0].count, 1);
        assert_eq!(
            snapshot.projects[1].count, 1,
            "exact cwd falls back to its project root"
        );
        assert_eq!(snapshot.projects[2].name, "无项目对话");
        assert_eq!(snapshot.chats.len(), 3, "subagent threads are hidden");
        assert_eq!(
            snapshot
                .chats
                .iter()
                .filter(|chat| chat.project_id == "p1")
                .count(),
            1
        );
        assert_eq!(
            snapshot
                .chats
                .iter()
                .find(|chat| chat.id == "t2")
                .unwrap()
                .project_id,
            "synthetic:uncategorized",
            "explicit projectless state overrides assignments and cwd"
        );
        assert_eq!(
            snapshot
                .chats
                .iter()
                .filter(|chat| chat.project_id == "synthetic:uncategorized")
                .count(),
            1
        );
        let assigned = snapshot.chats.iter().find(|chat| chat.id == "t1").unwrap();
        assert_eq!(assigned.title, "Visible");
        assert_eq!(assigned.updated_at, 2000);
        assert!(assigned.codex_pinned);
        assert_eq!(assigned.codex_unread, Some(true));
        assert_eq!(
            snapshot
                .chats
                .iter()
                .find(|chat| chat.id == "t2")
                .unwrap()
                .codex_unread,
            Some(false)
        );
        assert!(
            !snapshot
                .chats
                .iter()
                .find(|chat| chat.id == "t2")
                .unwrap()
                .codex_pinned
        );
        let state_path = directory.path().join(".codex-global-state.json");
        let mut state: serde_json::Value =
            serde_json::from_slice(&fs::read(&state_path).unwrap()).unwrap();
        state["electron-thread-read-state-v1"]["unreadByIdentity"]["identity-a"]
            ["local:account-a"] = serde_json::json!([]);
        fs::write(&state_path, state.to_string()).unwrap();
        assert!(scan_path(&database, directory.path())
            .unwrap()
            .chats
            .iter()
            .all(|chat| chat.codex_unread == Some(false)));
        state
            .as_object_mut()
            .unwrap()
            .remove("electron-thread-read-state-v1");
        fs::write(&state_path, state.to_string()).unwrap();
        assert!(scan_path(&database, directory.path())
            .unwrap()
            .chats
            .iter()
            .all(|chat| chat.codex_unread.is_none()));

        let mut previous = snapshot;
        previous
            .chats
            .iter_mut()
            .find(|chat| chat.id == "t1")
            .unwrap()
            .working = true;
        let connection = Connection::open(&database).unwrap();
        connection
            .execute(
                "UPDATE threads SET name='Renamed', archived=1 WHERE id='t1'",
                [],
            )
            .unwrap();
        connection.execute("INSERT INTO threads VALUES('t4', 'New', NULL, 1, 6, 6, 0, '', 'vscode', 'user', 'C:\\Pane', NULL)", []).unwrap();
        state["pinned-thread-ids"] = serde_json::json!([]);
        fs::write(&state_path, state.to_string()).unwrap();
        let updated =
            scan_path_with_previous(&database, directory.path(), Some(&previous)).unwrap();
        let renamed = updated.chats.iter().find(|chat| chat.id == "t1").unwrap();
        assert_eq!(renamed.title, "Renamed");
        assert!(renamed.archived && renamed.working);
        assert!(!renamed.codex_pinned);
        assert!(updated.chats.iter().any(|chat| chat.id == "t4"));
    }

    #[test]
    fn rejects_partial_schema() {
        let directory = tempdir().unwrap();
        let database = directory.path().join("state_5.sqlite");
        Connection::open(&database)
            .unwrap()
            .execute("CREATE TABLE threads(id TEXT PRIMARY KEY)", [])
            .unwrap();
        let error = scan_path(&database, directory.path()).unwrap_err();
        assert!(error.contains("Unsupported Codex schema"));
    }

    #[test]
    fn reads_current_thread_read_state() {
        let state: GlobalState = serde_json::from_value(serde_json::json!({
            "local-projects": {},
            "electron-thread-read-state-v1": {
                "unreadByIdentity": {
                    "identity-a": {
                        "local:account-a": ["current"],
                        "remote:account-a": ["remote"]
                    },
                    "identity-b": {"local:account-b": ["other"]}
                }
            }
        }))
        .unwrap();
        let unread = state.thread_read_state.local_unread().unwrap();
        assert!(unread.contains("current"));
        assert!(unread.contains("other"));
        assert!(!unread.contains("remote"));
    }

    #[test]
    fn reads_message_time_and_latest_turn_activity() {
        let directory = tempdir().unwrap();
        let database = directory.path().join("thread_history_1.sqlite");
        let connection = Connection::open(&database).unwrap();
        connection
            .execute_batch(
                "CREATE TABLE thread_turns(
               thread_id TEXT, turn_id TEXT, rollout_ordinal INTEGER, status TEXT,
               started_at INTEGER, completed_at INTEGER, duration_ms INTEGER, first_user_item_id TEXT
             );
             CREATE TABLE thread_items(thread_id TEXT, item_type TEXT, created_at_ms INTEGER);
             INSERT INTO thread_turns VALUES('active', '1', 1, 'completed', 110, 119, 10000, 'u1');
             INSERT INTO thread_turns VALUES('active', '2', 2, 'inProgress', 120, NULL, NULL, 'u2');
             INSERT INTO thread_turns VALUES('idle', '1', 1, 'completed', 130, 133, 3456, 'u3');
             INSERT INTO thread_turns VALUES('stale', '1', 1, 'inProgress', 90, NULL, NULL, 'u4');
             INSERT INTO thread_turns VALUES('stopped', '1', 1, 'interrupted', 140, 142, 2000, 'u5');
             INSERT INTO thread_items VALUES('active', 'userMessage', 121000);
             INSERT INTO thread_items VALUES('idle', 'userMessage', 131000);
             INSERT INTO thread_items VALUES('stale', 'userMessage', 91000);
             INSERT INTO thread_items VALUES('stopped', 'userMessage', 141000);",
            )
            .unwrap();
        drop(connection);

        let activity = read_thread_activity(&database, Some(100)).unwrap();
        assert!(activity["active"].working);
        assert_eq!(activity["active"].last_user_message_at, 121000);
        assert_eq!(activity["idle"].execution_ms, Some(3456));
        assert!(!activity["stale"].working);
        assert_eq!(activity["stale"].execution_status, None);
        assert_eq!(activity["stopped"].execution_ms, None);
        let connection = Connection::open(&database).unwrap();
        for id in ["active", "idle", "stale", "stopped"] {
            let targeted = read_thread_activity_for(&connection, id, Some(100), false)
                .unwrap()
                .unwrap();
            assert_eq!(targeted.working, activity[id].working);
            assert_eq!(targeted.execution_status, activity[id].execution_status);
            assert_eq!(targeted.execution_ms, activity[id].execution_ms);
            assert_eq!(
                targeted.last_user_message_at,
                activity[id].last_user_message_at
            );
        }
    }

    #[test]
    fn scans_old_chats_without_history_and_keeps_explicit_failure() {
        let directory = tempdir().unwrap();
        let home = directory.path();
        fs::write(
            home.join(".codex-global-state.json"),
            r#"{"local-projects":{}}"#,
        )
        .unwrap();
        let database = home.join("state_5.sqlite");
        let connection = Connection::open(&database).unwrap();
        connection.execute_batch("CREATE TABLE threads(id TEXT, title TEXT, name TEXT, created_at INTEGER, updated_at INTEGER, archived INTEGER, rollout_path TEXT, source TEXT, thread_source TEXT, cwd TEXT, project_id TEXT);").unwrap();
        for id in ["old", "failed"] {
            let path = home.join(format!("{id}.jsonl"));
            fs::write(&path, format!("{{\"type\":\"event_msg\",\"payload\":{{\"type\":\"task_complete\",\"turn_id\":\"{id}-turn\",\"started_at\":10,\"completed_at\":20,\"duration_ms\":10000}}}}\n")).unwrap();
            connection
                .execute(
                    "INSERT INTO threads VALUES(?1, ?1, NULL, 1, 2, 1, ?2, 'vscode', 'user', '', NULL)",
                    rusqlite::params![id, path.to_str().unwrap()],
                )
                .unwrap();
        }
        let history = Connection::open(home.join("thread_history_1.sqlite")).unwrap();
        history.execute_batch("CREATE TABLE thread_turns(thread_id TEXT, turn_id TEXT, rollout_ordinal INTEGER, status TEXT, started_at INTEGER, completed_at INTEGER, duration_ms INTEGER, first_user_item_id TEXT); CREATE TABLE thread_items(thread_id TEXT,item_type TEXT,created_at_ms INTEGER); INSERT INTO thread_turns VALUES('failed','failed-turn',1,'failed',10,20,10000,'u1');").unwrap();
        let snapshot = scan_path(&database, home).unwrap();
        assert_eq!(
            snapshot
                .chats
                .iter()
                .find(|chat| chat.id == "old")
                .unwrap()
                .execution_status
                .as_deref(),
            Some("completed")
        );
        assert_eq!(
            snapshot
                .chats
                .iter()
                .find(|chat| chat.id == "failed")
                .unwrap()
                .execution_status
                .as_deref(),
            Some("failed")
        );
        history
            .execute_batch("ALTER TABLE thread_turns ADD COLUMN error_json TEXT;")
            .unwrap();
        history
            .execute(
                "UPDATE thread_turns SET error_json=?1",
                [r#"{"codexErrorInfo":"usageLimitExceeded","message":"real quota failure"}"#],
            )
            .unwrap();
        let detailed = scan_path(&database, home).unwrap();
        let issue = detailed
            .chats
            .iter()
            .find(|chat| chat.id == "failed")
            .unwrap()
            .diagnostic
            .as_ref()
            .unwrap();
        assert_eq!(issue.kind, "quota");
        assert_eq!(issue.message, "real quota failure");
    }

    #[test]
    #[ignore = "audits all local Codex metadata"]
    fn live_snapshot_audit() {
        for _ in 0..2 {
            let started = std::time::Instant::now();
            let snapshot = scan().unwrap();
            let mut counts = std::collections::BTreeMap::new();
            let mut diagnostics = std::collections::BTreeMap::new();
            for chat in &snapshot.chats {
                if let Some(issue) = &chat.diagnostic {
                    *diagnostics.entry(issue.kind.as_str()).or_insert(0usize) += 1;
                }
                *counts
                    .entry(chat.execution_status.as_deref().unwrap_or("unknown"))
                    .or_insert(0usize) += 1;
                assert_eq!(
                    chat.working,
                    chat.execution_status.as_deref() == Some("inProgress")
                );
            }
            println!(
                "scan_ms={} chats={} states={counts:?} diagnostics={diagnostics:?} quota={:?}",
                started.elapsed().as_millis(),
                snapshot.chats.len(),
                snapshot.rate_limits
            );
        }
    }

    #[test]
    fn no_response_requires_observation_and_clears_after_progress() {
        let mut activity = ThreadActivity {
            working: true,
            observed: true,
            last_user_message_at: 1000,
            ..Default::default()
        };
        activity.update_diagnostic(None, 120999);
        assert!(activity.diagnostic.is_none());
        activity.update_diagnostic(None, 121000);
        assert_eq!(activity.diagnostic.as_ref().unwrap().kind, "noResponse");
        assert_eq!(activity.diagnostic.as_ref().unwrap().severity, "error");
        activity.last_response_at = 122000;
        activity.update_diagnostic(None, 123000);
        assert!(activity.diagnostic.is_none());
        activity.last_user_message_at = 130000;
        activity.waiting_for_tool = true;
        activity.update_diagnostic(None, 300000);
        assert!(activity.diagnostic.is_none());
        activity.waiting_for_tool = false;
        activity.observed = false;
        activity.update_diagnostic(None, 300000);
        assert!(activity.diagnostic.is_none());
        activity.observed = true;
        activity.update_diagnostic(None, 250000);
        assert_eq!(activity.diagnostic.as_ref().unwrap().kind, "noResponse");
        activity.working = false;
        activity.update_diagnostic(None, 300000);
        assert!(activity.diagnostic.is_none());
        activity.working = true;
        let retry = Diagnostic {
            kind: "retry".into(),
            severity: "warning".into(),
            at: 140000,
            ..Diagnostic::no_response(1000)
        };
        activity.update_diagnostic(Some(&retry), 300000);
        assert_eq!(activity.diagnostic.as_ref().unwrap().kind, "noResponse");
        activity.last_response_at = 150000;
        activity.update_diagnostic(Some(&retry), 260000);
        assert!(activity.diagnostic.is_none());
    }

    #[test]
    fn completed_duration_uses_recorded_time_or_valid_boundaries() {
        assert_eq!(
            execution_duration(Some(12345), Some(1000), Some(20000)),
            Some(12345)
        );
        assert_eq!(
            execution_duration(None, Some(1000), Some(13000)),
            Some(12000)
        );
        assert_eq!(execution_duration(None, None, Some(13000)), None);
        assert_eq!(execution_duration(None, Some(13000), Some(1000)), None);
        let directory = tempdir().unwrap();
        let file = directory.path().join("completed.jsonl");
        fs::write(&file, "{\"type\":\"event_msg\",\"payload\":{\"type\":\"task_complete\",\"turn_id\":\"done\",\"started_at\":120,\"completed_at\":132}}\n").unwrap();
        assert_eq!(
            read_rollout_snapshot(&file, Some(100))
                .unwrap()
                .0
                .unwrap()
                .execution_ms,
            Some(12000)
        );
        let file = directory.path().join("unknown.jsonl");
        fs::write(&file, "{\"type\":\"event_msg\",\"timestamp\":\"1970-01-01T00:02:12Z\",\"payload\":{\"type\":\"task_complete\",\"turn_id\":\"unknown\"}}\n").unwrap();
        assert_eq!(
            read_rollout_snapshot(&file, Some(100))
                .unwrap()
                .0
                .unwrap()
                .execution_ms,
            None
        );
    }

    #[test]
    fn rollout_overrides_stale_history_status() {
        let directory = tempdir().unwrap();
        let rollout = directory.path().join("rollout.jsonl");
        fs::write(
            &rollout,
            concat!(
                "{\"type\":\"event_msg\",\"payload\":{\"type\":\"task_started\",\"started_at\":120}}\n",
                "{\"type\":\"event_msg\",\"payload\":{\"type\":\"task_complete\",\"started_at\":120,\"completed_at\":125,\"duration_ms\":5000}}\n",
                "{\"type\":\"event_msg\",\"payload\":{\"type\":\"task_started\",\"started_at\":140}}\n"
            ),
        )
        .unwrap();

        let activity = read_rollout_snapshot(&rollout, Some(100))
            .unwrap()
            .0
            .unwrap();
        assert!(activity.working);
        assert_eq!(activity.last_user_message_at, 140000);
        assert_eq!(activity.execution_started_at, Some(140000));
        assert_eq!(activity.activity_at, 140000);
    }

    #[test]
    #[ignore = "reads local Codex data; requires CODEX_PANE_VERIFY_THREAD"]
    fn live_archived_chat_snapshot() {
        let id = env::var("CODEX_PANE_VERIFY_THREAD").unwrap();
        for _ in 0..2 {
            let snapshot = scan().unwrap();
            let chat = snapshot.chats.iter().find(|chat| chat.id == id).unwrap();
            assert!(chat.archived, "Source must report the archived thread");
            println!("archived={} project_id={}", chat.archived, chat.project_id);
        }
    }

    #[test]
    #[ignore = "reads local Codex data; requires CODEX_PANE_VERIFY_THREAD"]
    fn live_snapshot_tracks_rollout() {
        let id = env::var("CODEX_PANE_VERIFY_THREAD").unwrap();
        for _ in 0..2 {
            let started = std::time::Instant::now();
            let snapshot = scan().unwrap();
            let chat = snapshot.chats.iter().find(|chat| chat.id == id).unwrap();
            println!(
                "scan_ms={} working={} status={:?} last_user_message_at={} codex_pinned={} codex_unread={:?}",
                started.elapsed().as_millis(),
                chat.working,
                chat.execution_status,
                chat.last_user_message_at,
                chat.codex_pinned,
                chat.codex_unread
            );
            assert!(chat.working, "the verification thread is currently running");
            assert!(chat.execution_started_at.is_some());
        }
    }
}
