use serde_json::{json, Value};
use std::{
    collections::HashMap,
    env,
    fs::{self, File, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    process::Command,
    sync::{
        atomic::{AtomicU64, Ordering},
        Mutex, OnceLock,
    },
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

const MAX_FRAME_BYTES: usize = 8 * 1024 * 1024;
const DISCOVERY_TIMEOUT: Duration = Duration::from_secs(20);
const ACTIVATION_TIMEOUT: Duration = Duration::from_secs(20);
const VERIFY_TIMEOUT: Duration = Duration::from_secs(3);

static ACTION_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
static REQUEST_ID: AtomicU64 = AtomicU64::new(1);

pub fn run_action(
    thread_id: &str,
    action: &str,
    title: Option<&str>,
    enabled: Option<bool>,
    settle_delay_ms: u64,
) -> Result<(), String> {
    let _guard = ACTION_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .map_err(|_| "Codex MCP 操作锁已损坏".to_string())?;
    let (tool, arguments) = action_request(thread_id, action, title, enabled)?;
    let (pipe_path, namespace) = wait_for_tool(tool, DISCOVERY_TIMEOUT)?;
    let mut client = RpcClient::connect(&pipe_path)?;
    if let Err(error) = call_tool(&mut client, &namespace, tool, thread_id, arguments.clone()) {
        if !renderer_activation_required(&error) {
            return Err(error);
        }
        let mut log_offsets = snapshot_log_offsets();
        open_thread(thread_id)?;
        wait_for_active_renderer(thread_id, &mut log_offsets, ACTIVATION_TIMEOUT)?;
        thread::sleep(Duration::from_millis(settle_delay_ms.clamp(1_000, 5_000)));
        let (current_pipe, current_namespace) = wait_for_tool(tool, DISCOVERY_TIMEOUT)?;
        let mut retry_client = RpcClient::connect(&current_pipe)?;
        call_tool(
            &mut retry_client,
            &current_namespace,
            tool,
            thread_id,
            arguments,
        )?;
    }
    verify_action(thread_id, action, title, enabled)
}

pub fn run_project_pin(
    project_id: &str,
    enabled: bool,
    settle_delay_ms: u64,
) -> Result<(), String> {
    let _guard = ACTION_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .map_err(|_| "Codex MCP 操作锁已损坏".to_string())?;
    let context_thread = crate::source::scan()?
        .chats
        .first()
        .map(|chat| chat.id.clone())
        .ok_or_else(|| "没有可用于调用 Codex MCP 的对话".to_string())?;
    let tool = "move_project_to_sidebar_section";
    let arguments =
        json!({"projectId": project_id, "sectionId": if enabled { Some("pinned") } else { None }});
    let (pipe_path, namespace) = wait_for_tool(tool, DISCOVERY_TIMEOUT)?;
    let mut client = RpcClient::connect(&pipe_path)?;
    if let Err(error) = call_tool(
        &mut client,
        &namespace,
        tool,
        &context_thread,
        arguments.clone(),
    ) {
        if !renderer_activation_required(&error) {
            return Err(error);
        }
        let mut log_offsets = snapshot_log_offsets();
        open_thread(&context_thread)?;
        wait_for_active_renderer(&context_thread, &mut log_offsets, ACTIVATION_TIMEOUT)?;
        thread::sleep(Duration::from_millis(settle_delay_ms.clamp(1_000, 5_000)));
        let (current_pipe, current_namespace) = wait_for_tool(tool, DISCOVERY_TIMEOUT)?;
        let mut retry_client = RpcClient::connect(&current_pipe)?;
        call_tool(
            &mut retry_client,
            &current_namespace,
            tool,
            &context_thread,
            arguments,
        )?;
    }
    let deadline = Instant::now() + VERIFY_TIMEOUT;
    loop {
        if crate::source::scan()?
            .projects
            .iter()
            .any(|project| project.id == project_id && project.codex_pinned == enabled)
        {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err("Codex MCP 已返回成功，但重新读取后状态尚未更新".into());
        }
        thread::sleep(Duration::from_millis(250));
    }
}

fn renderer_activation_required(error: &str) -> bool {
    error.contains("-32000") || error.to_ascii_lowercase().contains("renderer")
}

fn action_request(
    thread_id: &str,
    action: &str,
    title: Option<&str>,
    enabled: Option<bool>,
) -> Result<(&'static str, Value), String> {
    match action {
        "rename" => {
            let title = title
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .ok_or_else(|| "新对话标题不能为空".to_string())?;
            if title.chars().count() > 256 {
                return Err("新对话标题不能超过 256 个字符".into());
            }
            Ok((
                "set_thread_title",
                json!({"threadId": thread_id, "title": title}),
            ))
        }
        "pin" => Ok((
            "move_thread_to_sidebar_section",
            json!({"threadId": thread_id, "sectionId": if enabled == Some(true) { Some("pinned") } else { None }}),
        )),
        "archive" => Ok((
            "set_thread_archived",
            json!({"threadId": thread_id, "archived": enabled.unwrap_or(true)}),
        )),
        _ => Err(format!("不支持的 Codex 操作：{action}")),
    }
}

fn open_thread(thread_id: &str) -> Result<(), String> {
    crate::open_uri(&format!("codex://threads/{thread_id}"))
}

fn wait_for_tool(tool: &str, timeout: Duration) -> Result<(String, String), String> {
    let deadline = Instant::now() + timeout;
    let mut saw_pipe = false;
    let mut last_error = None;
    loop {
        let paths = discover_pipe_paths();
        saw_pipe |= !paths.is_empty();
        for path in paths {
            match required_tool_namespace(&path, tool) {
                Ok(namespace) => return Ok((path, namespace)),
                Err(error) => last_error = Some(error),
            }
        }
        if Instant::now() >= deadline {
            return Err(last_error.unwrap_or_else(|| {
                if saw_pipe {
                    format!("当前 Codex 未提供 MCP 工具：{tool}")
                } else {
                    "未发现 Codex App Tools MCP 管道；请确认 Codex 桌面版正在运行".into()
                }
            }));
        }
        thread::sleep(Duration::from_millis(400));
    }
}

fn discover_pipe_paths() -> Vec<String> {
    let script = r#"
$lines = @(
  Get-CimInstance Win32_Process -Filter "Name = 'codex.exe'" |
    Where-Object { $_.CommandLine -like '*CODEX_APP_TOOLS_PIPE_PATH*' } |
    Select-Object -ExpandProperty CommandLine
  Get-ChildItem '\\.\pipe\' -ErrorAction SilentlyContinue |
    Where-Object { $_.Name -like 'codex-browser-use-*' } |
    Select-Object -ExpandProperty FullName
)
$lines
"#;
    let output = Command::new("powershell.exe")
        .args([
            "-NoLogo",
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            script,
        ])
        .output();
    output
        .ok()
        .filter(|output| output.status.success())
        .map(|output| parse_pipe_paths(&String::from_utf8_lossy(&output.stdout)))
        .unwrap_or_default()
}

fn parse_pipe_path(command_line: &str) -> Option<String> {
    let marker = "codex-browser-use-";
    let start = command_line.rfind(marker)?;
    let name: String = command_line[start..]
        .chars()
        .take_while(|ch| ch.is_ascii_alphanumeric() || *ch == '-')
        .collect();
    (!name.is_empty()).then(|| format!(r"\\.\pipe\{name}"))
}

fn parse_pipe_paths(output: &str) -> Vec<String> {
    let mut paths = Vec::new();
    for line in output.lines() {
        if let Some(path) = parse_pipe_path(line) {
            if !paths.contains(&path) {
                paths.push(path);
            }
        }
    }
    paths
}

fn required_tool_namespace(pipe_path: &str, required: &str) -> Result<String, String> {
    let mut client = RpcClient::connect(pipe_path)?;
    let result = client.request("tools/list", json!({"threadStartKind": "all"}))?;
    tool_namespace(&result, required)
        .ok_or_else(|| format!("当前 Codex 未提供 MCP 工具：{required}"))
}

fn tool_namespace(result: &Value, required: &str) -> Option<String> {
    result
        .get("tools")
        .and_then(Value::as_array)
        .and_then(|tools| {
            tools
                .iter()
                .find(|tool| tool.get("name").and_then(Value::as_str) == Some(required))
        })
        .and_then(|tool| tool.get("namespace").and_then(Value::as_str))
        .map(str::to_string)
}

fn call_tool(
    client: &mut RpcClient,
    namespace: &str,
    tool: &str,
    thread_id: &str,
    arguments: Value,
) -> Result<Value, String> {
    let suffix = request_suffix();
    let result = client.request(
        "tools/call",
        json!({
            "arguments": arguments,
            "callId": format!("codex-chat-pane-{suffix}"),
            "namespace": namespace,
            "threadId": thread_id,
            "tool": tool,
            "turnId": format!("codex-chat-pane-turn-{suffix}")
        }),
    )?;
    if result.get("success").and_then(Value::as_bool) != Some(true) {
        return Err(
            content_text(&result).unwrap_or_else(|| format!("Codex MCP 工具 {tool} 执行失败"))
        );
    }
    Ok(result)
}

fn verify_action(
    thread_id: &str,
    action: &str,
    title: Option<&str>,
    enabled: Option<bool>,
) -> Result<(), String> {
    let deadline = Instant::now() + VERIFY_TIMEOUT;
    loop {
        if let Ok(snapshot) = crate::source::scan() {
            if let Some(chat) = snapshot.chats.iter().find(|chat| chat.id == thread_id) {
                let verified = match action {
                    "rename" => title == Some(chat.title.as_str()),
                    "pin" => enabled == Some(chat.codex_pinned),
                    "archive" => enabled == Some(chat.archived),
                    _ => false,
                };
                if verified {
                    return Ok(());
                }
            }
        }
        if Instant::now() >= deadline {
            return Err("Codex MCP 已返回成功，但重新读取后状态尚未更新".into());
        }
        thread::sleep(Duration::from_millis(250));
    }
}

#[cfg(test)]
fn action_verified(
    snapshot: &Value,
    thread_id: &str,
    action: &str,
    title: Option<&str>,
    enabled: Option<bool>,
) -> bool {
    let threads = snapshot
        .get("threads")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let pinned = snapshot
        .get("pinnedThreads")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let matches = |row: &&Value| row.get("id").and_then(Value::as_str) == Some(thread_id);
    match action {
        "rename" => {
            threads
                .iter()
                .chain(&pinned)
                .find(matches)
                .and_then(|row| row.get("title").and_then(Value::as_str))
                == title
        }
        "pin" if enabled == Some(true) => pinned.iter().any(|row| matches(&row)),
        "pin" => !pinned.iter().any(|row| matches(&row)) && threads.iter().any(|row| matches(&row)),
        "archive" => {
            threads.iter().any(|row| matches(&row)) || pinned.iter().any(|row| matches(&row))
        }
        _ => false,
    }
}

fn content_text(result: &Value) -> Option<String> {
    result
        .get("contentItems")?
        .as_array()?
        .iter()
        .filter_map(|item| item.get("text").and_then(Value::as_str))
        .next()
        .map(str::to_string)
}

fn request_suffix() -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let id = REQUEST_ID.fetch_add(1, Ordering::Relaxed);
    format!("{now}-{id}")
}

struct RpcClient(File);

impl RpcClient {
    fn connect(path: &str) -> Result<Self, String> {
        OpenOptions::new()
            .read(true)
            .write(true)
            .open(path)
            .map(Self)
            .map_err(|error| format!("无法连接 Codex MCP 管道：{error}"))
    }

    fn request(&mut self, method: &str, params: Value) -> Result<Value, String> {
        let id = REQUEST_ID.fetch_add(1, Ordering::Relaxed);
        let payload = serde_json::to_vec(
            &json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}),
        )
        .map_err(|error| error.to_string())?;
        if payload.len() > MAX_FRAME_BYTES {
            return Err("Codex MCP 请求过大".into());
        }
        self.0
            .write_all(&(payload.len() as u32).to_le_bytes())
            .map_err(|error| error.to_string())?;
        self.0
            .write_all(&payload)
            .map_err(|error| error.to_string())?;
        self.0.flush().map_err(|error| error.to_string())?;

        loop {
            let mut header = [0_u8; 4];
            self.0
                .read_exact(&mut header)
                .map_err(|error| format!("Codex MCP 管道已断开：{error}"))?;
            let length = u32::from_le_bytes(header) as usize;
            if length > MAX_FRAME_BYTES {
                return Err("Codex MCP 响应过大".into());
            }
            let mut frame = vec![0_u8; length];
            self.0
                .read_exact(&mut frame)
                .map_err(|error| format!("Codex MCP 响应不完整：{error}"))?;
            let response: Value = serde_json::from_slice(&frame)
                .map_err(|error| format!("Codex MCP 响应无效：{error}"))?;
            if response.get("id").and_then(Value::as_u64) != Some(id) {
                continue;
            }
            if let Some(error) = response.get("error") {
                let code = error
                    .get("code")
                    .and_then(Value::as_i64)
                    .unwrap_or_default();
                let message = error
                    .get("message")
                    .and_then(Value::as_str)
                    .unwrap_or("Codex App Tool 请求失败");
                return Err(format!("Codex MCP {code}: {message}"));
            }
            return response
                .get("result")
                .cloned()
                .ok_or_else(|| "Codex MCP 响应缺少 result".into());
        }
    }
}

fn logs_root() -> Option<PathBuf> {
    let packages = PathBuf::from(env::var_os("LOCALAPPDATA")?).join("Packages");
    fs::read_dir(packages)
        .ok()?
        .filter_map(Result::ok)
        .find(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with("OpenAI.Codex_")
        })
        .map(|entry| entry.path().join("LocalCache/Local/Codex/Logs"))
}

fn recent_logs() -> Vec<PathBuf> {
    let Some(root) = logs_root() else {
        return Vec::new();
    };
    let mut files = Vec::new();
    collect_logs(&root, 4, &mut files);
    files.sort_by_key(|path| {
        fs::metadata(path)
            .and_then(|meta| meta.modified())
            .unwrap_or(UNIX_EPOCH)
    });
    files.into_iter().rev().take(16).collect()
}

fn collect_logs(path: &Path, depth: usize, output: &mut Vec<PathBuf>) {
    if depth == 0 {
        return;
    }
    let Ok(entries) = fs::read_dir(path) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_logs(&path, depth - 1, output);
        } else if path.extension().and_then(|value| value.to_str()) == Some("log") {
            output.push(path);
        }
    }
}

fn snapshot_log_offsets() -> HashMap<PathBuf, u64> {
    recent_logs()
        .into_iter()
        .filter_map(|path| fs::metadata(&path).ok().map(|meta| (path, meta.len())))
        .collect()
}

fn wait_for_active_renderer(
    thread_id: &str,
    offsets: &mut HashMap<PathBuf, u64>,
    timeout: Duration,
) -> Result<(), String> {
    let conversation = format!("conversationId={thread_id}");
    wait_for_renderer_line(
        offsets,
        timeout,
        |line| {
            line.contains("thread_stream_view_activity_changed active=true")
                && line.contains(&conversation)
                && line.contains("rendererWindowFocused=true")
                && line.contains("rendererWindowVisible=true")
        },
        "Codex 已打开，但未检测到目标对话的 active/focused/visible 日志信号",
    )
}

fn wait_for_renderer_line(
    offsets: &mut HashMap<PathBuf, u64>,
    timeout: Duration,
    matches: impl Fn(&str) -> bool,
    timeout_message: &str,
) -> Result<(), String> {
    let deadline = Instant::now() + timeout;
    loop {
        for path in recent_logs() {
            let Ok(meta) = fs::metadata(&path) else {
                continue;
            };
            let offset = offsets.entry(path.clone()).or_insert(0);
            if meta.len() < *offset {
                *offset = 0;
            }
            if meta.len() == *offset {
                continue;
            }
            let Ok(mut file) = File::open(&path) else {
                continue;
            };
            if file.seek(SeekFrom::Start(*offset)).is_err() {
                continue;
            }
            let mut appended = Vec::new();
            if file.read_to_end(&mut appended).is_err() {
                continue;
            }
            *offset = meta.len();
            let text = String::from_utf8_lossy(&appended);
            if text.lines().any(&matches) {
                return Ok(());
            }
        }
        if Instant::now() >= deadline {
            return Err(timeout_message.into());
        }
        thread::sleep(Duration::from_millis(200));
    }
}

#[cfg(test)]
mod tests {
    use super::{
        action_request, action_verified, parse_pipe_path, parse_pipe_paths,
        renderer_activation_required, tool_namespace,
    };
    use serde_json::json;

    #[test]
    fn parses_runtime_pipe_and_verifies_explicit_states() {
        assert!(renderer_activation_required(
            "Codex MCP -32000: renderer unavailable"
        ));
        assert!(!renderer_activation_required("validation failed"));
        let line =
            r#"env={\"CODEX_APP_TOOLS_PIPE_PATH\"=\"\\\\.\\pipe\\codex-browser-use-1234-abcd\"}"#;
        assert_eq!(
            parse_pipe_path(line).as_deref(),
            Some(r"\\.\pipe\codex-browser-use-1234-abcd")
        );
        let listing = r"\\.\pipe\codex-browser-use-stale
\\.\pipe\codex-browser-use-live
\\.\pipe\codex-browser-use-stale";
        assert_eq!(
            parse_pipe_paths(listing),
            vec![
                r"\\.\pipe\codex-browser-use-stale",
                r"\\.\pipe\codex-browser-use-live"
            ]
        );
        let tools =
            json!({"tools": [{"name": "set_thread_title", "namespace": "codex-app-tools"}]});
        assert_eq!(
            tool_namespace(&tools, "set_thread_title").as_deref(),
            Some("codex-app-tools")
        );
        let snapshot =
            json!({"threads": [{"id": "a", "title": "Renamed"}], "pinnedThreads": [{"id": "b"}]});
        assert!(action_verified(
            &snapshot,
            "a",
            "rename",
            Some("Renamed"),
            None
        ));
        assert!(action_verified(&snapshot, "b", "pin", None, Some(true)));
        assert_eq!(
            action_request("a", "archive", None, Some(true)).unwrap().0,
            "set_thread_archived"
        );
    }
}
