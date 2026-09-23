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

- The recent activity area at the top shows chat summaries from the last N days.
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

Use the preview control on a chat row to view its content. Pages are read from the corresponding rollout on demand; closing the preview does not save the text to tool settings.

### Organize folders

Create a local folder in Projects or Chats, then drag a project or chat into it. Folders, groups, order, and collapse state belong to CodexChatPane and are not written into Codex's project structure.

### Use groups

A group is a local, overlapping collection. A project or chat can belong to multiple groups. Deleting a group removes only the local relationship, not Codex data.

### Change Codex state

Renaming, pinning, archiving, and project pinning are Codex write operations:

1. Enable "Codex MCP operations" in settings.
2. Confirm consent before the first operation.
3. Choose the action from a chat or project menu.
4. Wait for completion and check the result message.

Archiving asks for another confirmation. An archived chat usually must be restored before it can be renamed or pinned.

## 4. Settings

The settings page provides:

- Chinese and English.
- Light and dark modes and theme families.
- Tab, pane, and row font sizes.
- Normal window, show with Codex, and always on top modes.
- Colored date bars.
- Log level.
- Codex MCP operations toggle.
- Tool settings import and export.

Local settings are in `settings.toml` under the Windows user config directory. Folder and group state is in `folders.json` in the same directory. The default directory is `%APPDATA%\com.codexchatpane.app\`. The app creates these files; they are not committed to the repository.

## 5. Troubleshooting

### The page says "Reading Codex data" or sync fails

Confirm Codex Desktop is running and its data directory is accessible. Use `CODEX_HOME` to point to a custom data directory. The app keeps the last valid snapshot instead of replacing it with an empty list.

### Browsing works, but rename or archive does not

Read-only browsing does not depend on MCP. Write operations require Codex MCP to be enabled in settings. If a Codex update changes its internal MCP, write operations may fail without affecting local folder organization.

### Reset local UI state

Close the app, then back up and move `settings.toml` and `folders.json` out of the user config directory. The next launch uses defaults and recreates the local settings.

### View debug information

Set the log level to `debug` in settings. Logs are written to the local CodexChatPane data directory and are not committed to the repository.
