use serde::Serialize;
use serde_json::Value;
use std::sync::Mutex;
use std::{collections::HashSet, path::Path};
use tauri::{Emitter, Manager};

fn require_enabled(app: &tauri::AppHandle) -> Result<(), String> {
    if crate::read_tool_config(app)?.is_some_and(|config| config.codex_radar_enabled) {
        Ok(())
    } else {
        Err("Unavailable".into())
    }
}

#[derive(Default)]
pub(crate) struct HoverState(Mutex<Hover>);
#[derive(Default)]
struct Hover {
    trigger: bool,
    panel: bool,
    revision: u64,
}
impl Hover {
    fn update(&mut self, surface: &str, inside: bool) -> Result<u64, String> {
        match surface {
            "trigger" => self.trigger = inside,
            "panel" => self.panel = inside,
            _ => return Err("Unknown radar surface".into()),
        }
        self.revision += 1;
        Ok(self.revision)
    }
    fn should_hide(&self, revision: u64) -> bool {
        self.revision == revision && !self.trigger && !self.panel
    }
}

pub(crate) fn hide_window(app: &tauri::AppHandle) {
    if let Ok(mut hover) = app.state::<HoverState>().0.lock() {
        hover.trigger = false;
        hover.panel = false;
        hover.revision += 1;
    }
    if let Some(popup) = app.get_webview_window("codex-radar") {
        let _ = popup.hide();
    }
    let _ = app.emit_to("main", "codex-radar-visibility", false);
}

#[tauri::command]
pub(crate) fn hide_codex_radar(app: tauri::AppHandle) {
    hide_window(&app);
}

#[tauri::command]
pub(crate) fn set_codex_radar_hover(
    app: tauri::AppHandle,
    surface: String,
    inside: bool,
) -> Result<(), String> {
    let revision = app
        .state::<HoverState>()
        .0
        .lock()
        .map_err(|e| e.to_string())?
        .update(&surface, inside)?;
    if !inside {
        // A short handoff lets the pointer cross between two native webviews.
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(140));
            let state = app.state::<HoverState>();
            let Ok(hover) = state.0.lock() else { return };
            if hover.should_hide(revision) {
                if let Some(popup) = app.get_webview_window("codex-radar") {
                    let _ = popup.hide();
                }
                let _ = app.emit_to("main", "codex-radar-visibility", false);
            }
        });
    }
    Ok(())
}

fn popup_bounds(
    anchor: (f64, f64, f64),
    area: (i32, i32, u32, u32),
    scale: f64,
) -> (i32, i32, u32, u32) {
    let pad = 4.0 * scale;
    let (left, top, w, h) = (area.0 as f64, area.1 as f64, area.2 as f64, area.3 as f64);
    let width = (400.0 * scale).min((w - 2.0 * pad).max(1.0));
    let height = (620.0 * scale).min((h - 2.0 * pad).max(1.0));
    let x = anchor
        .0
        .clamp(left + pad, (left + w - width - pad).max(left + pad));
    let proposed = if anchor.2 + height <= top + h - pad {
        anchor.2
    } else {
        anchor.1 - height
    };
    let y = proposed.clamp(top + pad, (top + h - height - pad).max(top + pad));
    (
        x.round() as i32,
        y.round() as i32,
        width.round() as u32,
        height.round() as u32,
    )
}

#[tauri::command]
pub(crate) async fn show_codex_radar(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    left: f64,
    top: f64,
    right: f64,
    bottom: f64,
) -> Result<(), String> {
    require_enabled(&app)?;
    if window.label() != "main"
        || ![left, top, right, bottom].iter().all(|v| v.is_finite())
        || right < left
        || bottom < top
    {
        return Err("Invalid radar anchor".into());
    }
    set_codex_radar_hover(app.clone(), "trigger".into(), true)?;
    let origin = window.outer_position().map_err(|e| e.to_string())?;
    let scale = window.scale_factor().map_err(|e| e.to_string())?;
    let anchor = (
        origin.x as f64 + left * scale,
        origin.y as f64 + top * scale,
        origin.y as f64 + bottom * scale,
    );
    let monitors = window.available_monitors().map_err(|e| e.to_string())?;
    let monitor = monitors
        .iter()
        .find(|m| {
            let p = m.position();
            let s = m.size();
            anchor.0 >= p.x as f64
                && anchor.0 < (p.x as f64 + s.width as f64)
                && anchor.1 >= p.y as f64
                && anchor.1 < (p.y as f64 + s.height as f64)
        })
        .or_else(|| monitors.first())
        .ok_or("No display available")?;
    let area = monitor.work_area();
    let bounds = popup_bounds(
        anchor,
        (
            area.position.x,
            area.position.y,
            area.size.width,
            area.size.height,
        ),
        monitor.scale_factor(),
    );
    let popup = if let Some(popup) = app.get_webview_window("codex-radar") {
        popup
    } else {
        let mut builder = tauri::WebviewWindowBuilder::new(
            &app,
            "codex-radar",
            tauri::WebviewUrl::App("index.html?radar=1".into()),
        )
        .title("CodexChatPane Radar")
        .inner_size(400.0, 620.0)
        .decorations(false)
        .resizable(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .focused(false)
        .visible(false)
        .drag_and_drop(false)
        .data_directory(crate::app_data_dir()?);
        if let Some(args) = crate::debug_browser_args(app.state::<crate::RemoteDebuggingPort>().0) {
            builder = builder.additional_browser_args(&args);
        }
        builder.build().map_err(|e| e.to_string())?
    };
    popup
        .set_size(tauri::PhysicalSize::new(bounds.2, bounds.3))
        .map_err(|e| e.to_string())?;
    popup
        .set_position(tauri::PhysicalPosition::new(bounds.0, bounds.1))
        .map_err(|e| e.to_string())?;
    let visible = {
        let state = app.state::<HoverState>();
        let hover = state.0.lock().map_err(|e| e.to_string())?;
        hover.trigger || hover.panel
    };
    if visible {
        popup.show().map_err(|e| e.to_string())?;
        let _ = popup.emit("codex-radar-open", ());
        let _ = app.emit_to("main", "codex-radar-visibility", true);
    }
    Ok(())
}

#[derive(Serialize)]
pub(crate) struct Model {
    id: String,
    name: String,
    efforts: Vec<String>,
}

fn read_models(home: &Path) -> Result<Vec<Model>, String> {
    let path = home.join("models_cache.json");
    let size = std::fs::metadata(&path)
        .map_err(|e| format!("本机 Codex 模型清单不可用：{e}"))?
        .len();
    if size > 4 * 1024 * 1024 {
        return Err("本机模型清单过大".into());
    }
    let value: Value = serde_json::from_slice(&std::fs::read(path).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    let rows = value["models"].as_array().ok_or("本机模型清单格式不支持")?;
    let mut seen = HashSet::new();
    let mut models = Vec::new();
    for row in rows {
        if row["visibility"].as_str() != Some("list") {
            continue;
        }
        let Some(id) = row["slug"].as_str().filter(|id| !id.is_empty()) else {
            continue;
        };
        if !seen.insert(id.to_owned()) {
            continue;
        }
        let mut efforts = Vec::new();
        for level in row["supported_reasoning_levels"]
            .as_array()
            .into_iter()
            .flatten()
        {
            if let Some(effort) = level["effort"].as_str().filter(|e| !e.is_empty()) {
                if !efforts.iter().any(|e| e == effort) {
                    efforts.push(effort.to_owned());
                }
            }
        }
        models.push(Model {
            id: id.into(),
            name: row["display_name"].as_str().unwrap_or(id).into(),
            efforts,
        });
    }
    Ok(models)
}

#[tauri::command]
pub(crate) fn get_codex_radar_models(app: tauri::AppHandle) -> Result<Vec<Model>, String> {
    require_enabled(&app)?;
    read_models(&crate::source::codex_home()?)
}

fn source_url(source: &str) -> Result<&'static str, String> {
    match source {
        "tests" => Ok("https://codexradar.com/data/codex-four-task-summary.json"),
        "ratings" => Ok("https://codexradar.com/api/model-ratings?view=public&window=7d"),
        "speed" => Ok(
            "https://speed-api.codexradar.com/api/agent-speed/public/models?window=5m&source=all",
        ),
        _ => Err("Unknown radar source".into()),
    }
}

fn read_source(source: &str) -> Result<Value, String> {
    let url = source_url(source)?;
    #[cfg(windows)]
    let executable = std::env::var_os("SystemRoot")
        .map(std::path::PathBuf::from)
        .ok_or("Windows directory unavailable")?
        .join("System32/curl.exe");
    #[cfg(not(windows))]
    let executable = std::path::PathBuf::from("curl");
    let mut command = std::process::Command::new(executable);
    command
        .args([
            "--disable",
            "--silent",
            "--fail",
            "--max-time",
            "15",
            "--connect-timeout",
            "5",
            "--max-filesize",
            "1048576",
            "--proto",
            "=https",
            "--user-agent",
            concat!("CodexChatPane/", env!("CARGO_PKG_VERSION")),
            "--header",
            "Accept: application/json",
            "--write-out",
            "\n%{http_code}",
            url,
        ])
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    let output = command
        .output()
        .map_err(|_| "Radar HTTP reader unavailable")?;
    if output.stdout.len() > 1_048_580 {
        return Err("Radar response too large".into());
    }
    let text =
        std::str::from_utf8(&output.stdout).map_err(|_| "Invalid radar response encoding")?;
    let (body, status) = text
        .rsplit_once('\n')
        .ok_or("Invalid radar HTTP response")?;
    if !output.status.success() || status != "200" {
        return Err(format!("HTTP {status} (curl {:?})", output.status.code()));
    }
    serde_json::from_str(body).map_err(|_| "Invalid radar JSON response".into())
}

#[tauri::command]
pub(crate) async fn read_codex_radar_source(
    app: tauri::AppHandle,
    source: String,
) -> Result<Value, String> {
    require_enabled(&app)?;
    tauri::async_runtime::spawn_blocking(move || read_source(&source))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub(crate) fn open_codex_radar(app: tauri::AppHandle) -> Result<(), String> {
    require_enabled(&app)?;
    crate::open_uri("https://codexradar.com")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn hover_handoff_and_screen_bounds_are_independent_of_pane() {
        let mut hover = Hover::default();
        hover.update("trigger", true).unwrap();
        let old = hover.update("trigger", false).unwrap();
        hover.update("panel", true).unwrap();
        assert!(!hover.should_hide(old));
        let out = hover.update("panel", false).unwrap();
        assert!(hover.should_hide(out));
        hover.update("trigger", true).unwrap();
        assert!(!hover.should_hide(out));
        let (x, y, w, h) = popup_bounds((130.0, 10.0, 40.0), (0, 0, 1920, 1040), 1.0);
        assert_eq!(x, 130);
        assert_eq!(y, 40);
        assert_eq!(w, 400);
        assert!(w > 320, "radar is not restricted to a narrow Pane");
        assert!(x + w as i32 <= 1920 && y + h as i32 <= 1040);
        let (x, y, w, h) = popup_bounds((-100.0, 850.0, 880.0), (-1280, 0, 1280, 900), 1.25);
        assert!(x >= -1280 && x + w as i32 <= 0 && y >= 0 && y + h as i32 <= 900);
    }
    #[test]
    fn visible_models_and_future_efforts_use_stable_ids() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("models_cache.json"), r#"{"models":[
          {"slug":"gpt-future-astra","display_name":"GPT-Future Astra","visibility":"list","supported_reasoning_levels":[{"effort":"new-effort"},{"effort":"low"},{"effort":"low"}]},
          {"slug":"gpt-hidden","visibility":"hide","supported_reasoning_levels":[]},
          {"slug":"gpt-future-sol","visibility":"list","supported_reasoning_levels":[{"effort":"max"}]}
        ]}"#).unwrap();
        let models = read_models(dir.path()).unwrap();
        assert_eq!(models.len(), 2);
        assert_eq!(models[0].id, "gpt-future-astra");
        assert_eq!(models[0].efforts, vec!["new-effort", "low"]);
        assert_eq!(models[1].name, "gpt-future-sol");
    }
    #[test]
    fn only_fixed_radar_sources_are_requested() {
        for source in ["tests", "ratings", "speed"] {
            assert!(source_url(source).unwrap().starts_with("https://"));
        }
        assert!(read_source("https://example.com").is_err());
    }
    #[test]
    #[ignore = "Reads three live public endpoints; run explicitly for integration acceptance"]
    fn live_sources() {
        for (source, field) in [("tests", "rows"), ("ratings", "models"), ("speed", "items")] {
            let started = std::time::Instant::now();
            let value = read_source(source).unwrap();
            assert_ne!(value["ok"], false);
            assert_ne!(value["demo"], true);
            let rows = value[field].as_array().expect("Expected source rows");
            println!(
                "radar source={source} rows={} status={} elapsed_ms={}",
                rows.len(),
                value["status"],
                started.elapsed().as_millis()
            );
        }
    }
}
