mod codex_app_mcp;
mod diagnostics;
mod preview;
mod source;
mod window_attach;

use serde::{Deserialize, Serialize};
use source::SourceSnapshot;
use std::{fs, process::Command, sync::Mutex};
use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, TrayIconBuilder, TrayIconEvent},
    Manager, WindowEvent,
};

#[derive(Default)]
struct SnapshotState(Mutex<Option<SourceSnapshot>>);

fn default_font_tab() -> f64 {
    15.0
}
fn default_font_pane() -> f64 {
    13.0
}
fn default_font_row() -> f64 {
    11.5
}
fn default_show_date_bars() -> bool {
    true
}

const FONT_MIN: f64 = 8.0;
const FONT_MAX: f64 = 20.0;
const WINDOW_WIDTH_MIN: f64 = 320.0;
const WINDOW_WIDTH_MAX: f64 = 4000.0;
const WINDOW_HEIGHT_MIN: f64 = 240.0;
const WINDOW_HEIGHT_MAX: f64 = 4000.0;

fn clamp_font(size: f64) -> Option<f64> {
    size.is_finite()
        .then(|| ((size * 2.0).round() / 2.0).clamp(FONT_MIN, FONT_MAX))
}

fn clamp_window_width(width: f64) -> Option<f64> {
    width
        .is_finite()
        .then(|| width.round().clamp(WINDOW_WIDTH_MIN, WINDOW_WIDTH_MAX))
}

fn clamp_window_height(height: f64) -> Option<f64> {
    height
        .is_finite()
        .then(|| height.round().clamp(WINDOW_HEIGHT_MIN, WINDOW_HEIGHT_MAX))
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default, rename_all = "camelCase")]
struct ToolConfig {
    language: String,
    theme: String,
    theme_family: String,
    window_pinned: bool,
    #[serde(default = "missing_window_mode")]
    window_mode: String,
    #[serde(default = "default_font_tab")]
    font_tab: f64,
    #[serde(default = "default_font_pane")]
    font_pane: f64,
    #[serde(default = "default_font_row")]
    font_row: f64,
    window_width: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    window_x: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    window_y: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    window_height: Option<f64>,
    #[serde(default = "default_show_date_bars")]
    show_date_bars: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    codex_mcp_enabled: Option<bool>,
    #[serde(default)]
    log_level: diagnostics::Level,
}

fn missing_window_mode() -> String {
    String::new()
}

impl Default for ToolConfig {
    fn default() -> Self {
        Self {
            language: "zh".into(),
            theme: "dark".into(),
            theme_family: "tokyo".into(),
            window_pinned: false,
            window_mode: "codex".into(),
            font_tab: default_font_tab(),
            font_pane: default_font_pane(),
            font_row: default_font_row(),
            window_width: None,
            window_x: None,
            window_y: None,
            window_height: None,
            show_date_bars: true,
            codex_mcp_enabled: None,
            log_level: diagnostics::Level::default(),
        }
    }
}

fn normalize_tool_config(mut config: ToolConfig) -> Result<ToolConfig, String> {
    if !matches!(config.language.as_str(), "zh" | "en")
        || !matches!(config.theme.as_str(), "light" | "dark")
        || !matches!(
            config.theme_family.as_str(),
            "catppuccin" | "tokyo" | "gruvbox" | "everforest" | "ayu"
        )
    {
        return Err("工具配置值无效".into());
    }
    if !matches!(config.window_mode.as_str(), "normal" | "codex" | "global") {
        return Err("工具配置值无效".into());
    }
    config.font_tab = clamp_font(config.font_tab).ok_or("工具配置值无效")?;
    config.font_pane = clamp_font(config.font_pane).ok_or("工具配置值无效")?;
    config.font_row = clamp_font(config.font_row).ok_or("工具配置值无效")?;
    if let Some(width) = config.window_width {
        config.window_width = Some(clamp_window_width(width).ok_or("工具配置值无效")?);
    }
    for coordinate in [&mut config.window_x, &mut config.window_y] {
        if let Some(value) = coordinate {
            if !value.is_finite() {
                return Err("工具配置值无效".into());
            }
            *value = value.round();
        }
    }
    if let Some(height) = config.window_height {
        config.window_height = Some(clamp_window_height(height).ok_or("工具配置值无效")?);
    }
    Ok(config)
}

fn legacy_config_path(name: &str) -> Result<std::path::PathBuf, String> {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(|root| root.join("config").join(name))
        .ok_or_else(|| "项目配置目录无效".into())
}

fn app_config_path(app: &tauri::AppHandle, name: &str) -> Result<std::path::PathBuf, String> {
    let directory = app
        .path()
        .app_config_dir()
        .map_err(|error| error.to_string())?;
    let path = directory.join(name);
    if path.exists() {
        return Ok(path);
    }

    let legacy = legacy_config_path(name)?;
    if legacy.exists() {
        fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
        fs::copy(&legacy, &path).map_err(|error| error.to_string())?;
    }
    Ok(path)
}

fn tool_config_path(app: &tauri::AppHandle) -> Result<std::path::PathBuf, String> {
    app_config_path(app, "settings.toml")
}

fn read_tool_config(app: &tauri::AppHandle) -> Result<Option<ToolConfig>, String> {
    let path = tool_config_path(app)?;
    if !path.exists() {
        return Ok(None);
    }
    toml::from_str(&fs::read_to_string(path).map_err(|error| error.to_string())?)
        .map(Some)
        .map_err(|error| error.to_string())
}

fn write_tool_config(app: &tauri::AppHandle, config: &ToolConfig) -> Result<(), String> {
    let path = tool_config_path(app)?;
    fs::create_dir_all(path.parent().ok_or("工具配置目录无效")?)
        .map_err(|error| error.to_string())?;
    fs::write(
        path,
        toml::to_string_pretty(config).map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())
}

#[tauri::command]
fn get_tool_config(app: tauri::AppHandle) -> Result<Option<ToolConfig>, String> {
    read_tool_config(&app)
}

#[tauri::command]
fn save_tool_config(
    app: tauri::AppHandle,
    logger: tauri::State<'_, diagnostics::Logger>,
    config: ToolConfig,
) -> Result<(), String> {
    let config = normalize_tool_config(config)?;
    write_tool_config(&app, &config)?;
    logger.set_level(config.log_level);
    logger.info(format!(
        "tool config saved logLevel={}",
        config.log_level.as_str()
    ));
    Ok(())
}

#[tauri::command]
async fn get_snapshot(app: tauri::AppHandle) -> SourceSnapshot {
    let result = tauri::async_runtime::spawn_blocking(source::scan)
        .await
        .map_err(|error| error.to_string())
        .and_then(|result| result);
    let state = app.state::<SnapshotState>();
    match result {
        Ok(snapshot) => {
            *state.0.lock().expect("snapshot lock poisoned") = Some(snapshot.clone());
            snapshot
        }
        Err(error) => {
            let mut guard = state.0.lock().expect("snapshot lock poisoned");
            if let Some(snapshot) = guard.as_mut() {
                snapshot.state = if error.contains("Unsupported") {
                    "Unsupported"
                } else {
                    "Stale"
                }
                .to_string();
                snapshot.error = Some(error);
                snapshot.clone()
            } else {
                SourceSnapshot {
                    state: if error.contains("Unsupported") {
                        "Unsupported"
                    } else {
                        "Unavailable"
                    }
                    .to_string(),
                    error: Some(error),
                    scope_key: String::new(),
                    host_id: "local".to_string(),
                    projects: Vec::new(),
                    chats: Vec::new(),
                    rate_limits: None,
                }
            }
        }
    }
}

#[tauri::command]
fn open_chat(
    thread_id: String,
    logger: tauri::State<'_, diagnostics::Logger>,
) -> Result<(), String> {
    logger.info(format!("open_chat requested threadId={thread_id}"));
    if !is_thread_id(&thread_id) {
        logger.warn("open_chat rejected invalid thread ID");
        return Err("Chat ID 格式无效".to_string());
    }
    let result = open_uri(&format!("codex://threads/{thread_id}"));
    match &result {
        Ok(()) => logger.debug("open_chat deep link dispatched"),
        Err(error) => logger.error(format!("open_chat deep link failed error={error}")),
    }
    result
}

#[tauri::command]
async fn run_codex_action(
    thread_id: String,
    action: String,
    title: Option<String>,
    enabled: Option<bool>,
    settle_delay_ms: Option<u64>,
) -> Result<(), String> {
    if !is_thread_id(&thread_id) {
        return Err("Chat ID 格式无效".to_string());
    }
    tauri::async_runtime::spawn_blocking(move || {
        codex_app_mcp::run_action(
            &thread_id,
            &action,
            title.as_deref(),
            enabled,
            settle_delay_ms.unwrap_or(1_000),
        )
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
fn new_chat(project_id: Option<String>, project_path: Option<String>) -> Result<(), String> {
    if project_id.as_deref().is_some_and(|id| !is_thread_id(id)) {
        return Err("Project ID 格式无效".into());
    }
    if project_path
        .as_deref()
        .is_some_and(|path| path.is_empty() || path.chars().any(char::is_control))
    {
        return Err("Project path 无效".into());
    }
    open_uri(&new_chat_uri(
        project_id.as_deref(),
        project_path.as_deref(),
    )?)
}

#[tauri::command]
async fn get_chat_preview(
    thread_id: String,
    before: Option<preview::PreviewCursor>,
    logger: tauri::State<'_, diagnostics::Logger>,
) -> Result<preview::PreviewPage, String> {
    let logger = logger.inner().clone();
    if !is_thread_id(&thread_id) {
        logger.warn(format!(
            "[DEBUG-preview] backend invalid threadId={thread_id}"
        ));
        return Err("Chat ID 格式无效".into());
    }
    let log_thread_id = thread_id.clone();
    let log_before = before;
    let started_at = std::time::Instant::now();
    logger.debug(format!(
        "[DEBUG-preview] backend request threadId={log_thread_id} before={log_before:?}"
    ));
    let result = tauri::async_runtime::spawn_blocking(move || preview::read(&thread_id, before))
        .await
        .map_err(|e| e.to_string())?;
    logger.debug(format!(
        "[DEBUG-preview] backend result threadId={log_thread_id} before={log_before:?} ok={} elapsedMs={}",
        result.is_ok(),
        started_at.elapsed().as_millis()
    ));
    result
}

#[tauri::command]
fn log_preview_event(logger: tauri::State<'_, diagnostics::Logger>, message: String) {
    let message = message.replace('\r', " ").replace('\n', " ");
    logger.debug(format!("[DEBUG-preview] frontend {message}"));
}

#[tauri::command]
fn open_project_directory(project_path: String) -> Result<(), String> {
    let path = std::path::Path::new(&project_path)
        .canonicalize()
        .map_err(|e| e.to_string())?;
    if !path.is_dir() {
        return Err("Project path 不是目录".into());
    }
    Command::new("explorer.exe")
        .arg(path)
        .spawn()
        .map(|_| ())
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn set_always_on_top(window: tauri::Window, enabled: bool) -> Result<(), String> {
    window
        .set_always_on_top(enabled)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn set_window_mode(
    window: tauri::Window,
    state: tauri::State<'_, window_attach::State>,
    tray_state: tauri::State<'_, window_attach::TrayState>,
    logger: tauri::State<'_, diagnostics::Logger>,
    mode: String,
) -> Result<(), String> {
    let value = match mode.as_str() {
        "normal" => 0,
        "codex" => 1,
        "global" => 2,
        _ => return Err("窗口模式无效".into()),
    };
    logger.info(format!("window mode requested mode={mode}"));
    let result = window_attach::set_mode(&window, &state, tray_state.inner(), &logger, value);
    if let Err(error) = &result {
        logger.error(format!("window mode failed mode={mode} error={error}"));
    }
    result
}

#[tauri::command]
async fn run_project_pin(project_id: String, enabled: bool) -> Result<(), String> {
    if !is_thread_id(&project_id) {
        return Err("Project ID 格式无效".into());
    }
    tauri::async_runtime::spawn_blocking(move || {
        codex_app_mcp::run_project_pin(&project_id, enabled, 1_000)
    })
    .await
    .map_err(|error| error.to_string())?
}

fn is_thread_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
}

fn encode_query_component(value: &str) -> String {
    let mut encoded = String::new();
    for &byte in value.as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                encoded.push(byte as char);
            }
            _ => encoded.push_str(&format!("%{byte:02X}")),
        }
    }
    encoded
}

fn new_chat_uri(project_id: Option<&str>, project_path: Option<&str>) -> Result<String, String> {
    if project_id.is_none() {
        return Ok("codex://threads/new".to_string());
    }
    Ok(project_path
        .filter(|path| !path.is_empty() && std::path::Path::new(path).is_dir())
        .map(|path| format!("codex://threads/new?path={}", encode_query_component(path)))
        .unwrap_or_else(|| "codex://threads/new".to_string()))
}

fn show_main(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

fn restore_main(app: &tauri::AppHandle) {
    if let Some(state) = app.try_state::<window_attach::TrayState>() {
        state.clear_user_closed();
        state.clear_auto_closed();
    }
    show_main(app);
}

fn focus_main(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.set_focus();
    }
}

fn handle_close_request(window: &tauri::Window) -> &'static str {
    let logger = window
        .try_state::<diagnostics::Logger>()
        .map(|state| state.inner().clone());
    if let Some(state) = window.app_handle().try_state::<window_attach::TrayState>() {
        state.mark_user_closed();
    }
    logger
        .as_ref()
        .map(|logger| logger.info("close request action=hide reason=user-closed"));
    if let Err(error) = window.hide() {
        logger
            .as_ref()
            .map(|logger| logger.error(format!("close request hide failed error={error}")));
    }
    "hidden"
}

#[tauri::command]
fn minimize_to_tray(window: tauri::Window) -> Result<(), String> {
    if let Some(state) = window.app_handle().try_state::<window_attach::TrayState>() {
        state.clear_user_closed();
        state.clear_auto_closed();
    }
    window.hide().map_err(|error| error.to_string())
}

fn setup_tray(app: &tauri::App) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, "open", "打开", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &quit])?;
    let icon = app
        .default_window_icon()
        .cloned()
        .or_else(|| tauri::image::Image::from_bytes(include_bytes!("../icons/icon.ico")).ok())
        .expect("tray icon");
    TrayIconBuilder::new()
        .icon(icon)
        .menu(&menu)
        .show_menu_on_left_click(false)
        .tooltip("CodexChatPane")
        .on_menu_event(|app, event| match event.id().as_ref() {
            "open" => restore_main(app),
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::DoubleClick {
                button: MouseButton::Left,
                ..
            } = event
            {
                restore_main(tray.app_handle());
            }
        })
        .build(app)?;
    Ok(())
}

#[tauri::command]
fn request_close(window: tauri::Window) -> Result<&'static str, String> {
    Ok(handle_close_request(&window))
}

fn folder_layout_path(app: &tauri::AppHandle) -> Result<std::path::PathBuf, String> {
    app_config_path(app, "folders.json")
}

#[tauri::command]
fn get_folder_layout(app: tauri::AppHandle) -> Result<Option<serde_json::Value>, String> {
    let path = folder_layout_path(&app)?;
    if !path.exists() {
        return Ok(None);
    }
    serde_json::from_str(&fs::read_to_string(path).map_err(|error| error.to_string())?)
        .map(Some)
        .map_err(|error| error.to_string())
}

fn json_to_toml(value: &serde_json::Value) -> Result<toml::Value, String> {
    Ok(match value {
        serde_json::Value::Null => return Err("TOML 不支持空值".into()),
        serde_json::Value::Bool(flag) => toml::Value::Boolean(*flag),
        serde_json::Value::Number(number) => {
            if let Some(integer) = number.as_i64() {
                toml::Value::Integer(integer)
            } else if let Some(unsigned) = number.as_u64() {
                toml::Value::Integer(i64::try_from(unsigned).map_err(|_| "数字无效".to_string())?)
            } else if let Some(float) = number.as_f64() {
                toml::Value::Float(float)
            } else {
                return Err("数字无效".into());
            }
        }
        serde_json::Value::String(text) => toml::Value::String(text.clone()),
        serde_json::Value::Array(items) => toml::Value::Array(
            items
                .iter()
                .map(json_to_toml)
                .collect::<Result<Vec<_>, _>>()?,
        ),
        serde_json::Value::Object(map) => {
            let mut table = toml::Table::new();
            for (key, child) in map {
                if child.is_null() {
                    continue;
                }
                table.insert(key.clone(), json_to_toml(child)?);
            }
            toml::Value::Table(table)
        }
    })
}

fn toml_to_json(value: &toml::Value) -> serde_json::Value {
    match value {
        toml::Value::String(text) => serde_json::Value::String(text.clone()),
        toml::Value::Integer(integer) => serde_json::json!(*integer),
        toml::Value::Float(float) => serde_json::json!(*float),
        toml::Value::Boolean(flag) => serde_json::Value::Bool(*flag),
        toml::Value::Datetime(datetime) => serde_json::Value::String(datetime.to_string()),
        toml::Value::Array(items) => {
            serde_json::Value::Array(items.iter().map(toml_to_json).collect())
        }
        toml::Value::Table(table) => {
            let mut map = serde_json::Map::new();
            for (key, child) in table {
                map.insert(key.clone(), toml_to_json(child));
            }
            serde_json::Value::Object(map)
        }
    }
}

fn unwrap_storage_field(
    data: &serde_json::Map<String, serde_json::Value>,
    key: &str,
) -> Result<Option<serde_json::Value>, String> {
    match data.get(key) {
        None | Some(serde_json::Value::Null) => Ok(None),
        Some(serde_json::Value::String(raw)) if key == "CodexChatPane.language" => {
            Ok(Some(serde_json::Value::String(raw.clone())))
        }
        Some(serde_json::Value::String(raw)) => serde_json::from_str(raw)
            .map(Some)
            .map_err(|error| error.to_string()),
        Some(value) => Ok(Some(value.clone())),
    }
}

fn normalize_export_json(value: serde_json::Value) -> Result<serde_json::Value, String> {
    let Some(root) = value.as_object() else {
        return Err("工具配置格式无效".into());
    };
    let Some(data) = root.get("data").and_then(|item| item.as_object()) else {
        return Ok(value);
    };
    let mut document = serde_json::Map::new();
    document.insert(
        "version".into(),
        serde_json::json!(root
            .get("version")
            .and_then(|item| item.as_u64())
            .unwrap_or(1)),
    );
    if let Some(language) = unwrap_storage_field(data, "CodexChatPane.language")? {
        document.insert("language".into(), language);
    }
    if let Some(preferences) = unwrap_storage_field(data, "CodexChatPane.preferences-v1")? {
        document.insert("preferences".into(), preferences);
    }
    if let Some(folders) = unwrap_storage_field(data, "CodexChatPane.folders-v1")? {
        document.insert("folders".into(), folders);
    }
    if let Some(dynamic) = unwrap_storage_field(data, "CodexChatPane.dynamic-v1")? {
        document.insert("dynamic".into(), dynamic);
    }
    Ok(serde_json::Value::Object(document))
}

fn exported_toml(payload: &serde_json::Value) -> Result<String, String> {
    if payload.get("version").and_then(|value| value.as_u64()) != Some(1) {
        return Err("工具配置格式无效".into());
    }
    let value = json_to_toml(payload)?;
    let table = value
        .as_table()
        .cloned()
        .ok_or_else(|| "工具配置格式无效".to_string())?;
    toml::to_string_pretty(&table).map_err(|error| error.to_string())
}

fn parse_exported_contents(contents: &str) -> Result<serde_json::Value, String> {
    if contents.len() > 5 * 1024 * 1024 {
        return Err("配置文件超过 5 MiB".into());
    }
    if contents.trim_start().starts_with('{') {
        let value: serde_json::Value =
            serde_json::from_str(contents).map_err(|error| error.to_string())?;
        normalize_export_json(value)
    } else {
        let table: toml::Table =
            toml::from_str(contents).map_err(|error: toml::de::Error| error.to_string())?;
        Ok(toml_to_json(&toml::Value::Table(table)))
    }
}

#[tauri::command]
fn export_config_file(payload: serde_json::Value) -> Result<Option<String>, String> {
    let toml = exported_toml(&payload)?;
    if toml.len() > 5 * 1024 * 1024 {
        return Err("配置文件超过 5 MiB".into());
    }
    let Some(path) = rfd::FileDialog::new()
        .set_title("导出工具配置")
        .set_file_name("CodexChatPane-config.toml")
        .add_filter("TOML", &["toml"])
        .save_file()
    else {
        return Ok(None);
    };
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    fs::write(&path, toml).map_err(|error| error.to_string())?;
    Ok(Some(path.to_string_lossy().into_owned()))
}

#[tauri::command]
fn parse_exported_config(contents: String) -> Result<serde_json::Value, String> {
    parse_exported_contents(&contents)
}

#[tauri::command]
fn save_folder_layout(app: tauri::AppHandle, layout: serde_json::Value) -> Result<(), String> {
    if !layout.is_object() {
        return Err("文件夹布局无效".into());
    }
    write_folder_layout(&folder_layout_path(&app)?, &layout)
}

fn write_folder_layout(path: &std::path::Path, layout: &serde_json::Value) -> Result<(), String> {
    if fs::read(path)
        .ok()
        .and_then(|contents| serde_json::from_slice(&contents).ok())
        .as_ref()
        == Some(layout)
    {
        return Ok(());
    }
    fs::create_dir_all(path.parent().ok_or("配置目录无效")?).map_err(|error| error.to_string())?;
    fs::write(
        path,
        serde_json::to_string_pretty(layout).map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())
}

pub(crate) fn open_uri(uri: &str) -> Result<(), String> {
    #[cfg(windows)]
    {
        win_shell::open(uri)
    }
    #[cfg(not(windows))]
    {
        let _ = uri;
        Err("Deep Link 仅支持 Windows".into())
    }
}

#[cfg(windows)]
mod win_shell {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;

    #[link(name = "shell32")]
    extern "system" {
        fn ShellExecuteW(
            hwnd: *mut core::ffi::c_void,
            lp_operation: *const u16,
            lp_file: *const u16,
            lp_parameters: *const u16,
            lp_directory: *const u16,
            n_show_cmd: i32,
        ) -> isize;
    }

    pub fn open(uri: &str) -> Result<(), String> {
        const SW_SHOWNORMAL: i32 = 1;
        let operation: Vec<u16> = OsStr::new("open").encode_wide().chain([0u16]).collect();
        let file: Vec<u16> = OsStr::new(uri).encode_wide().chain([0u16]).collect();
        let code = unsafe {
            ShellExecuteW(
                std::ptr::null_mut(),
                operation.as_ptr(),
                file.as_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                SW_SHOWNORMAL,
            )
        };
        if code > 32 {
            Ok(())
        } else {
            Err(format!("无法请求 Windows 打开 Deep Link：{code}"))
        }
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _, _| {
            focus_main(app);
        }))
        .manage(SnapshotState::default())
        .manage(window_attach::State::default())
        .manage(window_attach::TrayState::default())
        .setup(|app| {
            let logger = diagnostics::Logger::new(
                diagnostics::default_log_path(),
                diagnostics::Level::default(),
            );
            match read_tool_config(app.handle()) {
                Ok(Some(config)) => logger.set_level(config.log_level),
                Ok(None) => {}
                Err(error) => logger.warn(format!("tool config read failed error={error}")),
            }
            logger.info(format!(
                "app startup logLevel={} logPath={}",
                logger.level().as_str(),
                diagnostics::default_log_path().display()
            ));
            app.manage(logger.clone());
            setup_tray(app)?;
            window_attach::start(
                app.handle().clone(),
                app.state::<window_attach::State>().inner().clone(),
                app.state::<window_attach::TrayState>().inner().clone(),
                logger,
            );
            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                if window.label() == "conversation-preview" {
                    return;
                }
                api.prevent_close();
                handle_close_request(window);
            }
        })
        .invoke_handler(tauri::generate_handler![
            get_snapshot,
            get_tool_config,
            save_tool_config,
            open_chat,
            run_codex_action,
            new_chat,
            get_chat_preview,
            log_preview_event,
            open_project_directory,
            set_always_on_top,
            set_window_mode,
            run_project_pin,
            request_close,
            minimize_to_tray,
            get_folder_layout,
            save_folder_layout,
            export_config_file,
            parse_exported_config
        ])
        .run(tauri::generate_context!())
        .expect("error while running CodexChatPane");
}

#[cfg(test)]
mod tests {
    use super::diagnostics;
    use super::{
        encode_query_component, exported_toml, is_thread_id, new_chat_uri, normalize_tool_config,
        parse_exported_contents, write_folder_layout, ToolConfig,
    };

    #[test]
    fn validates_deep_link_identifier() {
        assert!(is_thread_id("01a05584-bd46-7ef1-8afc-b1d5b00cc6e6"));
        assert!(!is_thread_id("../bad?path"));
        assert_eq!(
            new_chat_uri(None, Some(r"D:\stale-project")).unwrap(),
            "codex://threads/new"
        );
        assert_eq!(
            new_chat_uri(Some("project-id"), Some(r"D:\missing-project")).unwrap(),
            "codex://threads/new"
        );
        let existing_project = std::env::current_dir().unwrap();
        let existing_project_path = existing_project.to_str().unwrap();
        assert!(
            !new_chat_uri(Some("project-id"), Some(existing_project_path))
                .unwrap()
                .contains("projectId"),
            "new chat must not use the obsolete projectId deep link"
        );
        assert_eq!(
            new_chat_uri(Some("project-id"), Some(existing_project_path)).unwrap(),
            format!(
                "codex://threads/new?path={}",
                encode_query_component(existing_project_path)
            )
        );
        let config = ToolConfig {
            language: "zh".into(),
            theme: "dark".into(),
            theme_family: "tokyo".into(),
            window_pinned: true,
            window_mode: "codex".into(),
            font_tab: 15.0,
            font_pane: 13.0,
            font_row: 11.5,
            window_width: Some(840.0),
            show_date_bars: true,
            codex_mcp_enabled: Some(true),
            ..ToolConfig::default()
        };
        let parsed = toml::from_str::<ToolConfig>(&toml::to_string(&config).unwrap()).unwrap();
        assert_eq!(parsed.theme_family, "tokyo");
        assert_eq!(parsed.font_tab, 15.0);
        assert_eq!(parsed.font_pane, 13.0);
        assert_eq!(parsed.font_row, 11.5);
        assert_eq!(parsed.window_width, Some(840.0));
        assert_eq!(parsed.window_mode, "codex");
        assert!(parsed.show_date_bars);
        assert_eq!(parsed.codex_mcp_enabled, Some(true));
        assert_eq!(parsed.log_level, diagnostics::Level::Info);
        let serialized = toml::to_string(&parsed).unwrap();
        assert!(serialized.contains("codexMcpEnabled = true"));
        assert!(!serialized.contains("closeToTray"));
        assert!(serialized.contains("logLevel = \"info\""));
        let legacy = toml::from_str::<ToolConfig>(
            "language = \"zh\"\ntheme = \"dark\"\nthemeFamily = \"tokyo\"\nwindowPinned = true\n",
        )
        .unwrap();
        assert_eq!(legacy.font_tab, 15.0);
        assert_eq!(legacy.font_pane, 13.0);
        assert_eq!(legacy.font_row, 11.5);
        assert_eq!(legacy.window_width, None);
        assert_eq!(legacy.window_mode, "");
        assert!(legacy.show_date_bars);
        assert_eq!(legacy.codex_mcp_enabled, None);
        assert_eq!(legacy.log_level, diagnostics::Level::Info);
        assert!(!toml::to_string(&legacy)
            .unwrap()
            .contains("codexMcpEnabled"));
    }

    #[test]
    fn clamps_out_of_range_tool_config_numbers() {
        let config = normalize_tool_config(ToolConfig {
            language: "zh".into(),
            theme: "dark".into(),
            theme_family: "tokyo".into(),
            window_pinned: true,
            window_mode: "codex".into(),
            font_tab: 7.0,
            font_pane: 21.0,
            font_row: 11.25,
            window_width: Some(252.0),
            show_date_bars: true,
            codex_mcp_enabled: None,
            ..ToolConfig::default()
        })
        .unwrap();
        assert_eq!(config.font_tab, 8.0);
        assert_eq!(config.font_pane, 20.0);
        assert_eq!(config.font_row, 11.5);
        assert_eq!(config.window_width, Some(320.0));
        let wide = normalize_tool_config(ToolConfig {
            language: "en".into(),
            theme: "light".into(),
            theme_family: "ayu".into(),
            window_pinned: false,
            window_mode: "normal".into(),
            font_tab: 15.0,
            font_pane: 13.0,
            font_row: 11.5,
            window_width: Some(5120.0),
            show_date_bars: true,
            codex_mcp_enabled: None,
            ..ToolConfig::default()
        })
        .unwrap();
        assert_eq!(wide.window_width, Some(4000.0));
        assert!(normalize_tool_config(ToolConfig {
            language: "".into(),
            theme: "dark".into(),
            theme_family: "tokyo".into(),
            ..ToolConfig::default()
        })
        .is_err());
    }

    #[test]
    fn unchanged_folder_layout_preserves_existing_file() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("folders.json");
        let compact = r#"{"openFolders":[]}"#;
        std::fs::write(&path, compact).unwrap();

        write_folder_layout(&path, &serde_json::json!({"openFolders": []})).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), compact);

        write_folder_layout(&path, &serde_json::json!({"openFolders": ["Work"]})).unwrap();
        assert!(std::fs::read_to_string(path).unwrap().contains("Work"));
    }

    #[test]
    fn exports_readable_toml_with_folders() {
        let payload = serde_json::json!({
            "version": 1,
            "language": "zh",
            "folders": {
                "projectFolders": ["Alpha"],
                "chatFolders": [{"project": "stock", "path": "Work"}],
                "projects": [{"id": "p1", "folder": "Alpha"}],
                "chats": [{"id": "c1", "folder": "Work"}]
            }
        });
        let toml = exported_toml(&payload).unwrap();
        assert!(toml.contains("language = \"zh\""));
        assert!(toml.contains("[folders]"));
        assert!(toml.contains("[[folders.projects]]") || toml.contains("Alpha"));
        assert!(!toml.contains("CodexChatPane.folders-v1"));
        assert!(!toml.contains("auto:"));
        let parsed = parse_exported_contents(&toml).unwrap();
        assert_eq!(parsed["language"], "zh");
        assert_eq!(parsed["folders"]["projectFolders"][0], "Alpha");
        assert_eq!(parsed["folders"]["projects"][0]["folder"], "Alpha");
        assert_eq!(parsed["folders"]["chats"][0]["folder"], "Work");
        assert_eq!(parsed["folders"]["chatFolders"][0]["path"], "Work");
    }

    #[test]
    fn parses_legacy_nested_json_export() {
        let legacy = serde_json::json!({
            "version": 1,
            "data": {
                "CodexChatPane.language": "en",
                "CodexChatPane.folders-v1": "{\"localProjectFolders\":[\"Beta\"]}"
            }
        });
        let parsed = parse_exported_contents(&legacy.to_string()).unwrap();
        assert_eq!(parsed["language"], "en");
        assert_eq!(parsed["folders"]["localProjectFolders"][0], "Beta");
    }
}
