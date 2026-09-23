# Architecture

[English](architecture.md) | [简体中文](architecture.zh-CN.md)

CodexChatPane is a Tauri 2 desktop app without a frontend framework. Rust handles local data, windows, and external operations; vanilla JavaScript handles state, rendering, and interaction.

## 1. Data flow

```text
Local Codex files
    │ read-only scan
    ▼
Rust SourceSnapshot
    │ Tauri invoke
    ▼
Frontend state ──> Project / Chat / Folder UI
    ▲                         │
    │                         ├─ Deep Link -> Codex Desktop
    │                         └─ App MCP  -> Codex write operations
    │
User config directory + localStorage
```

## 2. Code layers

### Rust / Tauri

- `src-tauri/src/lib.rs`: Tauri commands, window entry point, settings, and deep links.
- `src-tauri/src/source.rs`: reads Codex state and aggregates project and chat snapshots.
- `src-tauri/src/source/rollout.rs`: incrementally parses rollout activity, execution time, tokens, and usage limits.
- `src-tauri/src/source/diagnostics.rs`: reads a limited set of desktop diagnostic logs and classifies errors.
- `src-tauri/src/preview.rs`: reads conversation previews in pages with page and cache size limits.
- `src-tauri/src/codex_app_mcp.rs`: discovers, calls, and verifies Codex Desktop's internal App MCP.
- `src-tauri/src/window_attach.rs`: detects the foreground Windows window and coordinates pane visibility.

### Frontend

- `src/app.js`: app state, snapshot merging, sorting, rendering, and event handling.
- `src/dynamic.js`: groups, membership, collapse state, and stable ordering.
- `src/i18n.js`: runtime Chinese and English text.
- `src/styles.css`: layout, themes, windows, and controls.
- `index.html`: page shell and SVG icon library.

## 3. Startup and refresh

1. Tauri starts the main window and tray.
2. The frontend loads local settings and compatible `localStorage` state.
3. Rust scans Codex state, global state, and activity sources.
4. The frontend merges snapshots by stable ID and restores local folder and group relationships.
5. The main window refreshes snapshots on a fixed interval and updates only necessary state when the signature has not changed.
6. If the Rust scan fails, the app keeps the last valid snapshot and reports the error in the UI.

Before native data finishes loading, the app does not write empty frontend state back to local settings. This prevents a startup race from clearing the user's organization.

## 4. Snapshot boundary

The frontend mainly consumes these snapshot fields:

```text
state, error, scopeKey, hostId, projects, chats, rateLimits
```

Chat activity and diagnostics come from structured events and limited log reads. Rollout body text is excluded from normal snapshots. When changing a field, check Rust `serde` naming, frontend reads, and test fixtures together.

## 5. Persistence boundary

- Stable tool settings are written to `settings.toml` in the Tauri user config directory.
- Folders, groups, and local assignments are written to `folders.json` in the same directory.
- Frequently changing UI state is stored in browser `localStorage`.
- Original Codex databases, rollouts, and logs are opened read only.
- These config files are local runtime state and are not committed to the public repository.

## 6. Change guidelines

- Confirm data ownership before choosing where to write state.
- When adding a snapshot field, update Rust, the frontend, and fixtures or tests together.
- Do not guess a Codex schema in the frontend; show an error or degrade gracefully when uncertain.
- Keep shared sorting, classification, and status logic in one place.
- Run checks on both sides when changing the Rust/frontend contract.
