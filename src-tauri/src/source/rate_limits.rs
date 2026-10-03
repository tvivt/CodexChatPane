use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    io::{BufRead, BufReader, Write},
    process::{Child, Command, Stdio},
    sync::mpsc::{self, Receiver},
    thread,
    time::{Duration, Instant},
};

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct RateWindow {
    #[serde(alias = "usedPercent")]
    pub used_percent: f64,
    #[serde(alias = "windowDurationMins")]
    pub window_minutes: i64,
    #[serde(alias = "resetsAt")]
    pub resets_at: Option<i64>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct RateLimits {
    #[serde(alias = "limitId")]
    pub limit_id: Option<String>,
    pub primary: Option<RateWindow>,
    pub secondary: Option<RateWindow>,
    #[serde(alias = "planType")]
    pub plan_type: Option<String>,
}

fn response(lines: &Receiver<String>, id: i64, deadline: Instant) -> Result<Value, String> {
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err("Codex 额度查询超时".into());
        }
        let line = lines
            .recv_timeout(remaining)
            .map_err(|_| "Codex 额度查询超时或 app-server 已退出".to_string())?;
        let message: Value = serde_json::from_str(&line).map_err(|error| error.to_string())?;
        if message["id"].as_i64() != Some(id) {
            continue;
        }
        if let Some(error) = message.get("error") {
            return Err(error["message"]
                .as_str()
                .unwrap_or("Codex 额度查询失败")
                .to_string());
        }
        return Ok(message);
    }
}

fn parse_limits(message: &Value) -> Result<RateLimits, String> {
    let limits = message["rateLimitsByLimitId"]["codex"]
        .as_object()
        .or_else(|| message["rateLimits"].as_object())
        .ok_or("Codex 未返回额度")?;
    serde_json::from_value(Value::Object(limits.clone())).map_err(|error| error.to_string())
}

#[cfg(windows)]
fn installed_cli(bin: &std::path::Path) -> Option<std::path::PathBuf> {
    // ponytail: mtime selects cached versions; use a Desktop manifest if timestamps become unreliable.
    std::fs::read_dir(bin)
        .ok()?
        .flatten()
        .filter_map(|entry| {
            let path = entry.path().join("codex.exe");
            let metadata = path.metadata().ok()?;
            if !metadata.is_file() {
                return None;
            }
            Some((metadata.modified().ok()?, path))
        })
        .max_by_key(|(modified, _)| *modified)
        .map(|(_, path)| path)
}

fn cli_path() -> std::path::PathBuf {
    #[cfg(windows)]
    {
        if let Some(path) = super::desktop_app_server()
            .and_then(|(_, executable)| executable)
            .filter(|path| path.is_file())
        {
            return path;
        }
        if let Some(path) = std::env::var_os("LOCALAPPDATA").and_then(|local| {
            installed_cli(&std::path::PathBuf::from(local).join("OpenAI/Codex/bin"))
        }) {
            return path;
        }
    }
    "codex".into()
}

pub(super) fn read(thread_id: Option<&str>) -> Result<RateLimits, String> {
    let mcp = thread_id
        .ok_or_else(|| "没有可用的 MCP 对话上下文".to_string())
        .and_then(crate::codex_app_mcp::read_usage_limits)
        .and_then(|value| parse_limits(&value));
    mcp.or_else(|mcp_error| {
        read_cli().map_err(|cli_error| format!("MCP：{mcp_error}；CLI：{cli_error}"))
    })
}

fn read_cli() -> Result<RateLimits, String> {
    query(
        Command::new(cli_path()).arg("app-server"),
        Duration::from_secs(15),
        Duration::from_secs(2),
    )
}

fn query(
    command: &mut Command,
    timeout: Duration,
    exit_timeout: Duration,
) -> Result<RateLimits, String> {
    let deadline = Instant::now() + timeout;
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| format!("无法启动 Codex app-server：{error}"))?;
    #[cfg(windows)]
    let process_tree = ProcessTree::attach(&mut child)?;
    let result = (|| {
        let stdout = child.stdout.take().ok_or("Codex app-server 没有输出")?;
        let (sender, lines) = mpsc::channel();
        thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                if sender.send(line).is_err() {
                    break;
                }
            }
        });
        let mut stdin = child.stdin.take().ok_or("Codex app-server 没有输入")?;
        writeln!(stdin, "{}", json!({"method":"initialize","id":1,"params":{"clientInfo":{"name":"codex_chat_pane","title":"CodexChatPane","version":env!("CARGO_PKG_VERSION")}}})).map_err(|error| error.to_string())?;
        response(&lines, 1, deadline)?;
        writeln!(stdin, "{}", json!({"method":"initialized","params":{}}))
            .map_err(|error| error.to_string())?;
        writeln!(
            stdin,
            "{}",
            json!({"method":"account/rateLimits/read","id":2})
        )
        .map_err(|error| error.to_string())?;
        parse_limits(&response(&lines, 2, deadline)?["result"])
    })();
    if result.is_err() {
        #[cfg(windows)]
        process_tree.terminate();
        let _ = child.kill();
    }
    let exited = wait_for_exit(&mut child, Instant::now() + exit_timeout);
    if exited.is_err() {
        let _ = child.kill();
    }
    // The job also terminates descendants when the parent has already exited.
    #[cfg(windows)]
    drop(process_tree);
    result.and_then(|limits| exited.map(|()| limits))
}

fn wait_for_exit(child: &mut Child, deadline: Instant) -> Result<(), String> {
    loop {
        if child
            .try_wait()
            .map_err(|error| error.to_string())?
            .is_some()
        {
            return Ok(());
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err("Codex app-server 退出超时，已终止额度查询进程".into());
        }
        thread::sleep(remaining.min(Duration::from_millis(20)));
    }
}

#[cfg(windows)]
struct ProcessTree(std::os::windows::io::RawHandle);

#[cfg(windows)]
#[link(name = "kernel32")]
extern "system" {
    fn CreateJobObjectW(
        attributes: *const std::ffi::c_void,
        name: *const u16,
    ) -> std::os::windows::io::RawHandle;
    fn AssignProcessToJobObject(
        job: std::os::windows::io::RawHandle,
        process: std::os::windows::io::RawHandle,
    ) -> i32;
    fn TerminateJobObject(job: std::os::windows::io::RawHandle, exit_code: u32) -> i32;
    fn CloseHandle(handle: std::os::windows::io::RawHandle) -> i32;
}

#[cfg(windows)]
impl ProcessTree {
    fn attach(child: &mut Child) -> Result<Self, String> {
        use std::os::windows::io::AsRawHandle;
        unsafe {
            let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
            if job.is_null() || AssignProcessToJobObject(job, child.as_raw_handle()) == 0 {
                let error = std::io::Error::last_os_error();
                if !job.is_null() {
                    CloseHandle(job);
                }
                let _ = child.kill();
                return Err(format!("无法管理 Codex 额度查询进程：{error}"));
            }
            Ok(Self(job))
        }
    }

    fn terminate(&self) {
        unsafe { TerminateJobObject(self.0, 1) };
    }
}

#[cfg(windows)]
impl Drop for ProcessTree {
    fn drop(&mut self) {
        self.terminate();
        unsafe { CloseHandle(self.0) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn reads_codex_bucket_from_app_server() {
        let result = json!({"result":{"rateLimitsByLimitId":{"codex":{"limitId":"codex","primary":{"usedPercent":0.0,"windowDurationMins":10080,"resetsAt":1791081633},"secondary":null,"planType":"prolite"}}}});
        let limits = parse_limits(&result["result"]).unwrap();
        assert_eq!(limits.primary.as_ref().unwrap().window_minutes, 10080);
        assert_eq!(limits.primary.as_ref().unwrap().used_percent, 0.0);
        assert_eq!(limits.plan_type.as_deref(), Some("prolite"));
        assert_eq!(
            serde_json::to_value(limits).unwrap()["primary"]["window_minutes"],
            10080
        );
    }

    #[test]
    fn expired_deadline_rejects_buffered_responses() {
        let (sender, lines) = mpsc::channel();
        sender.send(r#"{"id":2,"result":{}}"#.into()).unwrap();
        assert!(response(&lines, 2, Instant::now())
            .unwrap_err()
            .contains("超时"));
    }

    #[cfg(windows)]
    #[test]
    fn discovers_cli_without_fixed_version_directory() {
        let root = tempfile::tempdir().unwrap();
        for (name, age) in [("old-version", 60), ("new-version", 0)] {
            let dir = root.path().join(name);
            std::fs::create_dir(&dir).unwrap();
            let file = std::fs::File::create(dir.join("codex.exe")).unwrap();
            file.set_times(
                std::fs::FileTimes::new()
                    .set_modified(std::time::SystemTime::now() - Duration::from_secs(age)),
            )
            .unwrap();
        }
        let unrelated = root.path().join("newest-rg-only");
        std::fs::create_dir(&unrelated).unwrap();
        std::fs::write(unrelated.join("rg.exe"), "").unwrap();
        assert_eq!(
            installed_cli(root.path()),
            Some(root.path().join("new-version/codex.exe"))
        );
        assert_eq!(installed_cli(&root.path().join("missing")), None);
    }

    #[cfg(windows)]
    #[test]
    fn stalled_queries_and_process_exits_are_bounded_and_retryable() {
        use std::os::windows::io::RawHandle;
        #[link(name = "kernel32")]
        extern "system" {
            fn OpenProcess(access: u32, inherit: i32, pid: u32) -> RawHandle;
            fn WaitForSingleObject(handle: RawHandle, milliseconds: u32) -> u32;
        }
        let root = tempfile::tempdir().unwrap();
        let script = r#"
$helper = Start-Process powershell.exe -WindowStyle Hidden -PassThru -ArgumentList '-NoProfile -NonInteractive -Command Start-Sleep -Seconds 60'
[IO.File]::WriteAllText($env:RATE_TEST_PIDS, "$PID`n$($helper.Id)")
if ($env:RATE_TEST_MODE -eq 'no_reply') { Start-Sleep -Seconds 60; exit }
$null = [Console]::ReadLine()
[Console]::WriteLine('{"id":1,"result":{}}')
$null = [Console]::ReadLine()
$null = [Console]::ReadLine()
[Console]::WriteLine('{"id":2,"result":{"rateLimits":{"primary":{"usedPercent":12,"windowDurationMins":300}}}}')
if ($env:RATE_TEST_MODE -eq 'no_exit') { Start-Sleep -Seconds 60 }
"#;
        for mode in ["no_reply", "no_exit", "normal"] {
            let pids = root.path().join(format!("{mode}.txt"));
            let started = Instant::now();
            let result = query(
                Command::new("powershell.exe")
                    .args(["-NoProfile", "-NonInteractive", "-Command", script])
                    .env("RATE_TEST_MODE", mode)
                    .env("RATE_TEST_PIDS", &pids),
                Duration::from_secs(5),
                Duration::from_millis(200),
            );
            assert!(started.elapsed() < Duration::from_secs(7), "{mode}");
            match mode {
                "no_reply" => assert!(result.unwrap_err().contains("查询超时")),
                "no_exit" => assert!(result.unwrap_err().contains("退出超时")),
                _ => assert_eq!(result.unwrap().primary.unwrap().used_percent, 12.0),
            }
            for pid in std::fs::read_to_string(pids).unwrap().lines() {
                unsafe {
                    let handle = OpenProcess(0x0010_0000, 0, pid.parse().unwrap()); // SYNCHRONIZE
                    if !handle.is_null() {
                        let status = WaitForSingleObject(handle, 2000);
                        CloseHandle(handle);
                        assert_eq!(status, 0, "process {pid} survived {mode}");
                    }
                }
            }
        }
    }

    #[cfg(windows)]
    #[test]
    #[ignore = "requires a signed-in Codex Desktop installation"]
    fn falls_back_to_live_cli_when_mcp_context_is_invalid() {
        let limits = read(Some("CodexChatPane-missing-context")).unwrap();
        assert!(limits.primary.is_some() || limits.secondary.is_some());
    }

    #[cfg(windows)]
    #[test]
    #[ignore = "requires a signed-in Codex Desktop installation"]
    fn reads_live_desktop_rate_limits() {
        let path = cli_path();
        assert!(path.is_absolute(), "Desktop CLI was not discovered");
        println!("Desktop CLI: {}", path.display());
        let limits = read_cli().unwrap();
        assert!(limits.primary.is_some() || limits.secondary.is_some());
    }
}
