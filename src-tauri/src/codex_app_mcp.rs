use serde_json::{json, Value};
use std::{
    collections::HashMap,
    env,
    fs::{self, File, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
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

pub fn read_usage_limits(thread_id: &str) -> Result<Value, String> {
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut last_error = "未发现可用的 Codex 额度 MCP 接口".to_string();
    for path in discover_pipe_paths() {
        if Instant::now() >= deadline {
            break;
        }
        let result = (|| {
            let probe_deadline = deadline.min(Instant::now() + Duration::from_millis(500));
            let mut client = RpcClient::connect_until(&path, probe_deadline)?;
            let tools = client.request("tools/list", json!({"threadStartKind": "all"}))?;
            let namespace = tool_namespace(&tools, "get_usage_limits")
                .ok_or("当前 Codex 未提供 get_usage_limits")?;
            client.deadline = deadline;
            let result = call_tool(
                &mut client,
                &namespace,
                "get_usage_limits",
                thread_id,
                json!({}),
            )?;
            let text = content_text(&result).ok_or("Codex MCP 未返回额度数据")?;
            serde_json::from_str(&text).map_err(|error| format!("无法解析 Codex MCP 额度：{error}"))
        })();
        match result {
            Ok(result) => return Ok(result),
            Err(error) => last_error = error,
        }
    }
    Err(last_error)
}

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
    let Ok(entries) = fs::read_dir(r"\\.\pipe\") else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            name.starts_with("codex-browser-use-")
                .then(|| parse_pipe_path(&name))
                .flatten()
        })
        .collect()
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
            "callerSource": "codex",
            "callId": format!("CodexChatPane-{suffix}"),
            "namespace": namespace,
            "threadId": thread_id,
            "tool": tool,
            "turnId": format!("CodexChatPane-turn-{suffix}")
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

struct RpcClient {
    pipe: File,
    deadline: Instant,
}

impl RpcClient {
    fn connect(path: &str) -> Result<Self, String> {
        Self::connect_until(path, Instant::now() + DISCOVERY_TIMEOUT)
    }

    fn connect_until(path: &str, deadline: Instant) -> Result<Self, String> {
        let pipe = OpenOptions::new()
            .read(true)
            .write(true)
            .open(path)
            .map_err(|error| format!("无法连接 Codex MCP 管道：{error}"))?;
        #[cfg(windows)]
        {
            use std::os::windows::io::{AsRawHandle, RawHandle};
            #[link(name = "kernel32")]
            extern "system" {
                fn SetNamedPipeHandleState(
                    pipe: RawHandle,
                    mode: *const u32,
                    count: *const u32,
                    timeout: *const u32,
                ) -> i32;
            }
            let mode = 1; // PIPE_NOWAIT: reads and writes must not block past the deadline.
            if unsafe {
                SetNamedPipeHandleState(
                    pipe.as_raw_handle(),
                    &mode,
                    std::ptr::null(),
                    std::ptr::null(),
                )
            } == 0
            {
                return Err(format!(
                    "无法设置 Codex MCP 管道超时模式：{}",
                    std::io::Error::last_os_error()
                ));
            }
        }
        Ok(Self { pipe, deadline })
    }

    fn read_exact(&mut self, mut buffer: &mut [u8]) -> Result<(), String> {
        while !buffer.is_empty() {
            let remaining = self.deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err("Codex MCP 请求超时".into());
            }
            #[cfg(windows)]
            let result = {
                use std::os::windows::io::{AsRawHandle, RawHandle};
                #[link(name = "kernel32")]
                extern "system" {
                    fn ReadFile(
                        pipe: RawHandle,
                        buffer: *mut u8,
                        size: u32,
                        read: *mut u32,
                        overlapped: *mut std::ffi::c_void,
                    ) -> i32;
                }
                let mut read = 0;
                // std::fs::File maps ERROR_NO_DATA to EOF; preserve it for PIPE_NOWAIT.
                if unsafe {
                    ReadFile(
                        self.pipe.as_raw_handle(),
                        buffer.as_mut_ptr(),
                        buffer.len() as u32,
                        &mut read,
                        std::ptr::null_mut(),
                    )
                } == 0
                {
                    Err(std::io::Error::last_os_error())
                } else {
                    Ok(read as usize)
                }
            };
            #[cfg(not(windows))]
            let result = self.pipe.read(buffer);
            match result {
                Ok(0) => return Err("Codex MCP 管道已断开".into()),
                Ok(count) => buffer = &mut buffer[count..],
                Err(error)
                    if error.raw_os_error() == Some(232) // ERROR_NO_DATA in PIPE_NOWAIT mode
                    || error.kind() == std::io::ErrorKind::WouldBlock =>
                {
                    thread::sleep(remaining.min(Duration::from_millis(10)));
                }
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(error) => return Err(format!("Codex MCP 管道读取失败：{error}")),
            }
        }
        Ok(())
    }

    fn request(&mut self, method: &str, params: Value) -> Result<Value, String> {
        if Instant::now() >= self.deadline {
            return Err("Codex MCP 请求超时".into());
        }
        let id = REQUEST_ID.fetch_add(1, Ordering::Relaxed);
        let payload = serde_json::to_vec(
            &json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}),
        )
        .map_err(|error| error.to_string())?;
        if payload.len() > MAX_FRAME_BYTES {
            return Err("Codex MCP 请求过大".into());
        }
        self.pipe
            .write_all(&(payload.len() as u32).to_le_bytes())
            .map_err(|error| error.to_string())?;
        self.pipe
            .write_all(&payload)
            .map_err(|error| error.to_string())?;

        loop {
            let mut header = [0_u8; 4];
            self.read_exact(&mut header)?;
            let length = u32::from_le_bytes(header) as usize;
            if length > MAX_FRAME_BYTES {
                return Err("Codex MCP 响应过大".into());
            }
            let mut frame = vec![0_u8; length];
            self.read_exact(&mut frame)?;
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
        action_request, action_verified, parse_pipe_path, renderer_activation_required,
        tool_namespace,
    };
    use serde_json::json;

    #[cfg(windows)]
    #[test]
    fn pipe_calls_include_caller_source_and_bound_stalled_or_invalid_replies() {
        use super::*;
        use std::os::windows::io::{AsRawHandle, FromRawHandle, RawHandle};
        #[link(name = "kernel32")]
        extern "system" {
            fn CreateNamedPipeW(
                name: *const u16,
                access: u32,
                mode: u32,
                instances: u32,
                out_size: u32,
                in_size: u32,
                timeout: u32,
                security: *const std::ffi::c_void,
            ) -> RawHandle;
            fn ConnectNamedPipe(pipe: RawHandle, overlapped: *mut std::ffi::c_void) -> i32;
        }
        for mode in ["reply", "stall", "invalid", "disconnect"] {
            let path = format!(
                r"\\.\pipe\CodexChatPane-test-{}-{}",
                std::process::id(),
                request_suffix()
            );
            let name: Vec<u16> = path.encode_utf16().chain(Some(0)).collect();
            let handle = unsafe {
                CreateNamedPipeW(name.as_ptr(), 3, 0, 1, 65536, 65536, 0, std::ptr::null())
            };
            assert_ne!(handle as isize, -1);
            let mut pipe = unsafe { File::from_raw_handle(handle) };
            let server = thread::spawn(move || {
                let connected =
                    unsafe { ConnectNamedPipe(pipe.as_raw_handle(), std::ptr::null_mut()) };
                assert!(
                    connected != 0 || std::io::Error::last_os_error().raw_os_error() == Some(535)
                );
                let mut header = [0; 4];
                pipe.read_exact(&mut header).unwrap();
                let mut body = vec![0; u32::from_le_bytes(header) as usize];
                pipe.read_exact(&mut body).unwrap();
                let request: Value = serde_json::from_slice(&body).unwrap();
                assert_eq!(request["params"]["callerSource"], "codex");
                assert_eq!(request["params"]["tool"], "get_usage_limits");
                if mode == "stall" {
                    thread::sleep(Duration::from_millis(350));
                    return;
                }
                if mode == "disconnect" {
                    return;
                }
                let response = if mode == "invalid" {
                    b"invalid JSON".to_vec()
                } else {
                    serde_json::to_vec(&json!({"id":request["id"], "result":{"success":true,"contentItems":[{"text":"{}"}]}})).unwrap()
                };
                let header = (response.len() as u32).to_le_bytes();
                pipe.write_all(&header[..2]).unwrap();
                thread::sleep(Duration::from_millis(10));
                pipe.write_all(&header[2..]).unwrap();
                pipe.write_all(&response).unwrap();
                // Keep the server handle alive until the client has consumed the reply.
                thread::sleep(Duration::from_millis(100));
            });
            let started = Instant::now();
            let timeout = if mode == "stall" {
                Duration::from_millis(100)
            } else {
                Duration::from_secs(2)
            };
            let mut client = RpcClient::connect_until(&path, started + timeout).unwrap();
            let result = call_tool(
                &mut client,
                "codex_app",
                "get_usage_limits",
                "test-context",
                json!({}),
            );
            match mode {
                "reply" => assert!(result.is_ok(), "{result:?}"),
                "stall" => {
                    assert!(result.unwrap_err().contains("超时"));
                    assert!(started.elapsed() < Duration::from_millis(300));
                }
                "invalid" => assert!(result.unwrap_err().contains("响应无效")),
                _ => assert!(result.is_err()),
            }
            drop(client);
            server.join().unwrap();
        }
    }

    #[cfg(windows)]
    #[test]
    #[ignore = "requires a running Codex Desktop and CODEX_THREAD_ID"]
    fn reads_live_usage_limits_over_mcp() {
        let id = std::env::var("CODEX_PANE_VERIFY_THREAD")
            .or_else(|_| std::env::var("CODEX_THREAD_ID"))
            .unwrap();
        let value = super::read_usage_limits(&id).unwrap();
        assert!(
            value["rateLimitsByLimitId"]["codex"]["primary"].is_object()
                || value["rateLimits"]["primary"].is_object()
        );
    }

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
