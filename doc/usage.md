# Usage guide

[English](usage.md) | [简体中文](usage.zh-CN.md)

This guide is for first-time CodexChatPane users.

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
- Chats without a project appear in a fixed "Project/Chats" category rather than being represented as a real project.

### Chats

The Chats area provides a global view for browsing and organizing conversations:

- `PIN` shows chats pinned in Codex.
- `ALL` shows available chats.
- Custom folders can be nested; a chat can be placed in a local folder.
- Search, project filtering, sorting, archive views, and multi-selection are available.

## 3. Common actions

### Open a chat

Click a chat row or its action menu to open the conversation in Codex Desktop. CodexChatPane does not copy the conversation text; it uses a deep link to locate the target.

### Preview a chat

Conversation preview is off by default. Enable it in settings to show the preview control on chat rows. Pages are read from the corresponding rollout on demand; closing the preview does not save the text to tool settings.

To override the recent activity filter, right-click a chat and choose **Activity display → Auto, Include, or Exclude**. The Activity rules dialog shows both override lists and lets you return a chat to Auto.

### View chat metrics

Selecting a chat reads its local logs once. Metrics follow the time as `1.8s·62t/s·93%`, using the latest completed turn: native first-token latency, estimated generation speed including reasoning, and input cache hit rate. Details defaults to the same turn; whole-conversation totals remain selectable. The time keeps its width; truncated metrics remain available in tips with sample coverage. Click the metrics or choose **Conversation details**, the last item in an ordinary chat's context menu, to open a separate resizable window with a draggable header and minimize, maximize, and close controls instead of a system titlebar.

Details show completed-turn totals or an individual turn, request exclusion reasons, activity records, tool times, and separate subagent summaries. Click a subagent to inspect its own records. Tool waits are excluded from generation speed; requests without reliable timing retain their usage but do not count toward speed. Missing values show `—`. Statistics refresh on another click, with no realtime polling. Archived menus still contain only **Restore chat**.

Quotas show `5h ring percentage HH:mm` and `W ring percentage`; only 5h shows its reset time in the status bar. Each tooltip retains its full reset date/time and countdown, such as `2026-10-09 02:30 (-5h42m)`. Countdowns longer than 24 hours use days and whole hours, such as `(-1d2h)`; shorter ones, including exactly 24 hours, retain hours and minutes.

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

## 5. Troubleshooting

### The page says "Reading Codex data" or sync fails

Confirm Codex Desktop is running and its data directory is accessible. Use `CODEX_HOME` to point to a custom data directory. The app keeps the last valid snapshot instead of replacing it with an empty list.

### Browsing works, but rename or archive does not

Read-only browsing does not depend on MCP. Write operations require Codex MCP to be enabled in settings. If a Codex update changes its internal MCP, write operations may fail without affecting local folder organization.

### Reset local UI state

Close the app, then back up and move `settings.toml` and `folders.json` out of `%CODEX_HOME%\.codex-chat-pane\`. The next launch uses defaults and recreates the local settings.

### View debug information

Set the log level to `debug` in settings. Logs are written to the local CodexChatPane data directory and are not committed to the repository.
