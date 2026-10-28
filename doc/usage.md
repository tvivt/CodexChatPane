# Usage guide

[English](usage.md) | [简体中文](usage.zh-CN.md)

This guide is for first-time CodexChatPane users.

The [complete usage reference](usage.zh-CN.md) includes a context-menu matrix, sorting rules, window geometry, and settings migration. Detailed numerical definitions are in [conversation metrics](conversation-metrics.zh-CN.md) and [quota and consumption](usage-history.zh-CN.md).

## 1. Start the app

For development, run these commands from the repository root:

```powershell
npm ci
npm run tauri dev
```

For a release build, run `CodexChatPane.exe` directly without installation. The target Windows machine needs WebView2 Runtime installed.

CodexChatPane reads local Codex data at startup. It does not create another Codex account or upload that data to a remote service.

Running only `npm run dev` starts a browser preview of the frontend. You can inspect the layout and some interactions, but it cannot read local Codex data, coordinate windows, or perform Codex actions.

## 2. Main window

### Projects

The Projects area shows recent conversations from a project perspective:

- The recent activity area at the top lists chats from the last N days. Its Activity rules button lists chats manually included or excluded from that view.
- The project structure area shows Codex projects and local folders.
- Selecting a project shows its chats in the adjacent or lower area.
- Chats without a project appear in a synthetic unprojected-chats category rather than a real project.

### Chats

The Chats area provides a global view for browsing and organizing conversations:

- `PIN` shows chats pinned in Codex.
- `ALL` shows available chats.
- Custom folders can be nested; a chat can be placed in a local folder.
- Search, project filtering, sorting, archive views, and multi-selection are available.

Status order is waiting for an answer, unread/completed awaiting review, working, then read. Only structured questions create a waiting state; matching replies resolve individual questions, and later ordinary user input invalidates earlier pending questions. An async question survives turn completion. Unsynchronized cold activity shows a neutral dot.

Waiting chats use the same `?` as projects, with “Waiting for answer” on hover. Only Projects has a close control, which collapses both Projects and Chats. Collapsed sections retain their names and a single upward restore icon on Projects. That icon restores both sections, preserving folder expansion, heights and the archive setting. Legacy independent collapse settings follow Projects.

P cycles project grouping off/ascending/descending; N cycles status/name ascending/name descending. One direction is active per control; status has no reverse mode. With P enabled, N orders chats within each project. Folder name sorting is independent. Groups may overlap and preserve view-specific order; Ctrl/Meta and Shift support multi-selection. Folder disclosure arrows and section restore controls preserve existing expansion state. Local stars differ from Codex pins.

## 3. Common actions

### Open a chat

Click a chat row or its action menu to open the conversation in Codex Desktop. CodexChatPane does not copy the conversation text; it uses a deep link to locate the target.

### Preview a chat

Conversation preview is off by default. Enable it in settings to show the preview control on chat rows. Pages are read from the corresponding rollout on demand; closing the preview does not save the text to tool settings.

To override the recent activity filter, right-click a chat and choose **Activity display → Auto, Include, or Exclude**. The Activity rules dialog shows both override lists and lets you return a chat to Auto.

### View chat metrics

Selecting a chat reads its current turn's latest valid settled request, including active/interrupted turns: `1.8s·62t/s·93%` means first-token latency, measured generation estimate including reasoning, and input cache hit rate. It does not average all requests. TTFT must belong to that request; later requests with no reliable boundary show `—`. Details defaults to the current turn, with reverse chronological turns and whole-conversation completed-turn totals available. Click the metrics or the ordinary chat menu's last item to open the separate resizable details window.

Details show scope-dependent activity counts, turn summaries in pages of fifty, tool totals, and direct subagent metadata. Request/activity detail lists are removed. Paging reuses one frozen summary snapshot and updates its section without reading logs; cold or changed-version opens may still parse the full target history. Clicking a subagent loads its own statistics without recursively loading children. Tool waits and inherited/child usage are excluded from the main speed. Invalid timing retains valid usage, with speed `—`. A new turn never inherits selected-chat single-request metrics. Archived menus still offer only Restore chat.

The separate consumption section includes all settled requests owned by this chat, including active/interrupted turns, with Token totals, API-equivalent cost and price-table date. Its scope does not change with the turn selector and excludes child usage. Missing timing does not discard valid usage. The user's local accounting policy assigns zero cost to gpt-reserve, codex-auto-review, gpt-5.3-codex-spark and records without a model name, retaining Tokens and request counts; this is not a promise of free service.

Σ sums running main-chat forecasts, each weighted by generation time over at most three recent valid requests with matching configuration. It displays `Σ≈125t/s`, `Σ0t/s` with no eligible chats, or `Σ—` when every eligible chat lacks a sample; there is no space after Σ. Tool execution, tool waits and silent reasoning retain the estimate without a silence timeout. Confirmed current compaction is excluded until it ends; completed, interrupted, failed, idle, waiting-for-answer and child chats are excluded. Tips show `Chat title[model]` on the left and right-aligned `Xt/s` without an approximation sign, or `—` when unknown, showing coverage when partial. Status updates retain the Σ hover/focus node. This predicts generation capacity, not actual Token arrivals per second, and does not replace selected-chat statistics or predict TTFT.

Quotas show `5h ring percentage HH:mm` and `W ring percentage`; only 5h shows its reset time in the status bar. Each tooltip retains its full reset date/time and countdown, such as `2026-10-09 02:30 (-5h42m)`. Countdowns longer than 24 hours use days and whole hours, such as `(-1d2h)`; shorter ones, including exactly 24 hours, retain hours and minutes.

Click quotas for reset history by default: current W plus five ended periods, ten more per load. Period labels include duration; text selects one period, checkboxes select multiple disjoint periods. Calendar day/week/month/all and reset periods are mutually exclusive. Consumption filters project/source/Fast; models are breakdowns, not a separate model filter. The overview shows five models by cost with expansion, the daily model stack defaults to Token and can switch to API-equivalent cost, and project/day/chat rows expand. Chat rows include user-question rounds and agents. Token is input+output; cached input and reasoning output are subsets. Reset details and persistent early-reset notices include period, remaining quota, model tokens/shares and API equivalents. All amounts are local estimates, not subscription charges.

Quota-window changes are distinct from reset events. Fixed/floating modes require consecutive observations; deadline-only changes remain candidates. First consumption never moves an identified period start. Labels, duration and consumption use the same `[start,end)` boundary, with stable dismissals after history refinement. Missing resets are not invented.

### Organize folders

Create a local folder in Projects or Chats, then drag a project or chat into it. Folders, groups, order, and collapse state belong to CodexChatPane and are not written into Codex's project structure.

### Use groups

A group is a local, overlapping collection. A project or chat can belong to multiple groups. Deleting a group removes only the local relationship, not Codex data.

### Change Codex state

Renaming, pinning, archiving, and project pinning are Codex write operations:

1. Enable "Codex MCP operations" in settings.
2. Confirm consent before the first operation.
3. Choose the action from a chat or project menu.
4. Wait for completion. The app shows a persistent notification if an error or warning occurs.

Archiving asks for another confirmation. An archived chat usually must be restored before it can be renamed or pinned.

## 4. Settings

The settings page provides:

- Chinese and English.
- Light and dark modes and theme families.
- Tab, pane, and row font sizes.
- Normal window, show with Codex, and always on top modes.
- Colored date bars.
- Conversation preview switch (off by default).
- Log level.
- Codex MCP operations toggle.
- Tool settings import and export.

Local settings are in `%CODEX_HOME%\.codex-chat-pane\settings.toml`. Folders, groups, stars, and Activity rules are in `folders.json` in the same directory. Without `CODEX_HOME`, the root is `%USERPROFILE%\.codex`. The app creates these files; they are not committed to the repository.

Positions use physical X/Y (including negative coordinates on a second monitor), dimensions use logical units, and restoration checks current monitor intersection. Maximization and auxiliary windows do not overwrite normal main-window geometry. Legacy windowPinned/localStorage keys migrate; invalid imports do not replace valid settings. Exports contain tool organization and settings, not credentials, logs, or statistics databases. Diagnostics offers an optional loopback debug port, off by default and effective after restart; forecasts do not use it.

Context menus are target-specific: chats offer activity overrides, rename/archive, folder move, locate project, open directory, copy title/link/diagnostic, and details; projects offer new chat, local folder move, directory, and identity copies. Multi-selection omits single-target actions. Custom folders support nesting/rename/delete; automatic folders only collapse. Group and blank-area menus control expansion, Timeline controls archive inclusion/closing, status menus copy time/quota, and titlebar/status background imports/exports settings. See the [complete menu table](usage.zh-CN.md#6-右键菜单速查) for restrictions. Local folder deletion changes organization, not Codex files or native archive state.

All context menus close on an outside click, application focus loss, or Escape. The opaque archive panel targets its own chat and offers only Restore chat; it does not open the menu of a row behind it. Fast appears only on turns whose recorded configuration enables it; normal/unknown settings have no badge. Details use a separate window with an internal draggable title area and window controls.

## 5. Troubleshooting

### The page says "Reading Codex data" or sync fails

Confirm Codex Desktop is running and its data directory is accessible. Use `CODEX_HOME` to point to a custom data directory. The app keeps the last valid snapshot instead of replacing it with an empty list.

### Browsing works, but rename or archive does not

Read-only browsing does not depend on MCP. Write operations require Codex MCP to be enabled in settings. If a Codex update changes its internal MCP, write operations may fail without affecting local folder organization.

### Reset local UI state

Close the app, then back up and move `settings.toml` and `folders.json` out of `%CODEX_HOME%\.codex-chat-pane\`. The next launch uses defaults and recreates the local settings.

### View debug information

Set the log level to `debug` in settings. Logs are written to the local CodexChatPane data directory and are not committed to the repository.
