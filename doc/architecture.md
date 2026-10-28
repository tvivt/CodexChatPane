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
CODEX_HOME/.codex-chat-pane + localStorage
```

## 2. Code layers

### Rust / Tauri

- `src-tauri/src/lib.rs`: Tauri commands, window entry point, settings, and deep links.
- `src-tauri/src/source.rs`: reads Codex state and aggregates project and chat snapshots.
- `src-tauri/src/source/rollout.rs`: incrementally parses rollout activity and execution time.
- `src-tauri/src/source/rate_limits.rs`: reads live usage limits through App MCP with a CLI app-server fallback and bounded process cleanup.
- `src-tauri/src/source/diagnostics.rs`: reads a limited set of desktop diagnostic logs and classifies errors.
- `src-tauri/src/source_watch.rs`: watches the Codex home root without recursion and only watches `sessions` / `archived_sessions` recursively.
- `src-tauri/src/preview.rs`: reads conversation previews in pages with page and cache size limits.
- `metrics.rs` / `metrics/latest.rs`: full own details and per-thread latest-request cursors; a separate three-request forecast leaves measured summaries unchanged.
- `metrics/details.rs`: four compact summary slots, fifty turns per page and frozen pagination revisions. Full accounting runs before request/activity bodies are released. Paging does not reread logs; cold or changed-version opens may still parse the full target history.
- `source/rollout/generation.rs`: own-thread turn/configuration evidence on the existing activity cursor; lifecycle remains owned by the established activity state.
- `metrics/analysis.rs`: one necessary own-usage/question/quota scan with persistent complete-line checkpoints. `usage/{context,costs,ledger}.rs` handle identity, consumption, and quota cycles.
- `usage/ledger/recognizer.rs`: compact observations become window modes, candidates and stable reset events. Persisted checkpoints/aliases live in the existing quota database. One adopted boundary drives labels and cached consumption regrouping; recognition never scans Token logs.
- `src-tauri/src/codex_app_mcp.rs`: discovers, calls, and verifies Codex Desktop's internal App MCP.
- `src-tauri/src/window_attach.rs`: detects the foreground Windows window and coordinates pane visibility.

### Frontend

- `src/app.js`: app state, snapshot merging, sorting, rendering, and event handling.
- `src/dynamic.js`: groups, membership, collapse state, and stable ordering.
- `src/usage.js` / `usage-preload.js`: quota window, notifications, cache-first reports and changed-ID queues for actual consumers; no startup period preloader.
- `src/i18n.js`: runtime Chinese and English text.
- `src/styles.css`: layout, themes, windows, and controls.
- `index.html`: page shell and SVG icon library.

## 3. Startup and refresh

1. Tauri starts the main window and tray.
2. The frontend loads local settings and compatible `localStorage` state.
3. Both snapshot commands use metadata-only `scan_catalog`: state/global metadata, no rollout/history bodies, diagnostics, project-path I/O or quota dependency. Cached state is preserved; cold unknown is neutral.
4. Render immediately, then defer history status and project-path validation through `get_chat_activity`. Initial projections bootstrap status; subsequent work targets active/waiting/selected/changed IDs. Fragment projections fall back to the logical thread if absent; `activityPending` carries new or relocated threads.
5. Catalog changes update list metadata and reuse activity caches; rollout changes update only the affected chat IDs. History changes compare a cached latest-turn status summary and supplement only changed IDs, including late failure details. Tool output and projection changes do not reload all rollouts.
   Active chats receive an incremental rollout / diagnostic check once per second without querying history; elapsed clocks use cached start times. An open preview reads its own content separately in pages and stops refreshing when hidden or closed.
   Hidden windows retain pending changes; becoming visible flushes those changes without a full history scan. Usage limits refresh independently at startup, on visibility resume, and once per minute while visible. Watcher overflow or recovery can trigger a full scan to restore lost events.
6. If the Rust scan fails, the app keeps the last valid snapshot and reports the error in the UI.

Before native data finishes loading, the app does not write empty frontend state back to local settings. This prevents a startup race from clearing the user's organization.

Snapshot I/O runs outside its short clone/version/commit lock; identity and epoch are rechecked before commit. Latest requests have per-thread locks rather than a global lock held through decompression. Consumption database indexing remains serialized and shared tasks are not cancelled for another consumer.

## 4. Snapshot boundary

The frontend mainly consumes these snapshot fields:

```text
state, error, projects, chats, activityPending
project: pathChecked, pathValid, error
chat: activitySynced, turnId, metricsRevision, generation
generation: turnId, phase, at, config {model, effort, serviceTier}
```

Chat activity and diagnostics come from structured events and limited log reads. Rollout body text is excluded from normal snapshots. When changing a field, check Rust `serde` naming, frontend reads, and test fixtures together.

## 5. Persistence boundary

- Stable tool settings are written to `%CODEX_HOME%\.codex-chat-pane\settings.toml`.
- `remoteDebuggingPort` defaults to `0` (off). Settings / Diagnostics accepts `0–65535`; a nonzero port enables loopback-only WebView2 CDP after a complete app restart. The main and preview windows share the startup arguments, including the existing browser defaults. Changing the saved port does not alter the running WebView environment.
- Folders, groups, stars, recent activity overrides, and local assignments are written to `folders.json` in the same directory. Without `CODEX_HOME`, the root is `%USERPROFILE%\.codex`.
- Frequently changing UI state is stored in browser `localStorage`.
- Original Codex databases, rollouts, and logs are opened read only.
- These config files are local runtime state and are not committed to the public repository.
- Analytics databases and short question summaries live in the app's own directory. Forecast phases/timers and complete message bodies are not persisted in settings.

Current cache limits are 32 latest thread slots, 24 preview pages, 32 chain metadata entries, 128 compressed logical lengths, and eight quota-cycle results. Entries are evicted individually. `usage-analysis-v1` checkpoints preserve parser/legacy baselines/dedup and safe offsets; normal JSONL append validates metadata, header and cursor guard before reading complete new lines. Replacement, truncation, compression, dependency/version changes rebuild. Reports use `details-v7` and explicit complete range coverage; partial scans cannot certify zero. Quota cycles invalidate by identity, live samples, import revision/month coverage, and dismissals; minute notifications use live-only history. Full definitions and residual in-place-tampering/expanded-compressed-preview limits are in the [cache table](architecture.zh-CN.md#缓存与失效).

Forecasts retain matching samples during tool waits and silence; rendering ticks use memory, without a silence timeout or historical I/O. Eligibility follows the existing running state, excluding confirmed current compaction. A historical compacted record or ZST file is not a compaction-start signal, so logs without that signal cannot guarantee full interval exclusion. Metadata timing is not whole-app readiness; first large/compressed reads can remain expensive. [Metrics](conversation-metrics.zh-CN.md) defines forecasts versus measurements; [consumption](usage-history.zh-CN.md) defines durable caches and consumer queues.

## 6. Change guidelines

- Confirm data ownership before choosing where to write state.
- When adding a snapshot field, update Rust, the frontend, and fixtures or tests together.
- Do not guess a Codex schema in the frontend; show an error or degrade gracefully when uncertain.
- Keep shared sorting, classification, and status logic in one place.
- Run checks on both sides when changing the Rust/frontend contract.
