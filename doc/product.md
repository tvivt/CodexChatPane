# Product model

[English](product.md) | [简体中文](product.zh-CN.md)

This document describes the current user-visible behavior and data boundaries of CodexChatPane. See [architecture.md](architecture.md) for implementation details and [integration.md](integration.md) for Codex integration.

## 1. Product goal

CodexChatPane makes local Codex Desktop projects and chats easier to browse, filter, and organize together. It provides a companion local view; it does not replace Codex Desktop or create another remote data source.

## 2. Core entities

| Entity | Owner | Meaning |
| --- | --- | --- |
| Project | Codex | Project identity, name, and path in Codex |
| Chat | Codex | Conversation identity, title, archive status, project, and timestamps |
| Folder | CodexChatPane | Local hierarchical organization |
| Group | CodexChatPane | Local collection that can overlap with other groups |
| Timeline | Codex events and rollouts | Recent activity, execution time, current state, and usage limits |
| Archive | Codex | Native Codex archive state, separate from the local folder recycle bin |

## 3. Data ownership

- Codex owns projects, chats, archives, Codex pins, and unread state.
- CodexChatPane owns local folders, groups, stars, manual order, themes, and window settings.
- Working state, execution time, usage limits, and diagnostics are derived from Codex data and events; the app does not rewrite original events.
- Conversation text is read on demand for previews and is not saved in local snapshots.

## 4. Project assignment for chats

Chat-to-project assignment follows this priority:

1. An explicit no-project marker.
2. Codex's global thread-to-project assignment.
3. The thread's own `project_id`.
4. A match between the thread's current working directory and a project path.
5. If none match, the chat goes into a synthetic "Unclassified chats" project.

The synthetic project is only a UI placeholder and is not written back to Codex.

## 5. Sorting and status

- By default, chats are ordered by activity area and received time; local click time is not treated as Codex activity.
- Name sorting, project filtering, date ranges, and archive views are available.
- Chat status can include unread, working, completed, failed, interrupted, and no-response diagnostics.
- Project and chat pin priority comes from Codex; local groups do not change Codex pins.

## 6. Local persistence

The app creates two kinds of local files:

- `settings.toml` in the user config directory: language, theme, font sizes, window mode and dimensions, log level, and MCP consent.
- `folders.json` in the same directory: folders, groups, local project and chat assignments, stars, order, and collapse state.

Filters, pane heights, and some frequently changing UI state are stored in browser `localStorage`. They are not synced to Codex.

## 7. Out of scope

- Uploading conversations, rollouts, logs, or local databases.
- Automatically converting local folders into Codex projects.
- Writing synthetic projects back to Codex.
- Promising compatibility with every future Codex Desktop version beyond its stable public APIs.
- Providing macOS or Linux window integration at present.
