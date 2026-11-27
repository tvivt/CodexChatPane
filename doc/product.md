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
| Timeline | Codex events, rollouts, and live usage queries | Recent activity, execution time, current state, and usage limits |
| Archive | Codex | Native Codex archive state, separate from the local folder recycle bin |

## 3. Data ownership

- Codex owns projects, chats, archives, Codex pins, and unread state.
- CodexChatPane owns local folders, groups, stars, recent activity rules, manual order, themes, and window settings.
- Working state, execution time, usage limits, and diagnostics are derived from Codex data and events; the app does not rewrite original events.
- Parsers may inspect message content temporarily for state/questions. Complete text is shown on demand in preview/details, not persisted in ordinary catalog snapshots or settings.
- Settled metrics and consumption are distinct from Σ predictions. Forecast phases/timers are memory-only; rebuildable own-request and short question-summary caches stay in the app's data directory.

## 4. Project assignment for chats

Chat-to-project assignment follows this priority:

1. An explicit no-project marker.
2. Codex's global thread-to-project assignment.
3. The thread's own `project_id`.
4. If no explicit assignment exists, the chat goes into a synthetic "Unclassified chats" project.

The synthetic project is only a UI placeholder and is not written back to Codex.

Global catalog settings prefer the `global` scope in `.codex-global-state.sqlite`; absent keys fall back to legacy `.codex-global-state.json`. Explicit empty settings and no-project markers remain authoritative. Working directories and project root paths never determine membership, including projects with multiple roots; paths remain available for display and opening folders.

Unread state, chat/project pins and project order use the same current global settings. Empty unread/pin sets override stale legacy values. Usage reports refresh cached project labels from a valid catalog without rereading rollout bodies or changing request usage; missing, stale or foreign catalogs preserve known cached assignments. A valid explicit no-project change can still clear a cached assignment. Pane's activity and local custom sorting continue to control its display order.

Chat titles observe both `threads.name` (falling back to `title`) and `session_index.jsonl`. During a running session, changes in either source update the title without requiring a new message; unchanged stale values do not undo an observed rename. On initial load, conflicting values use the index timestamp versus the thread timestamp. File notifications feed the existing 180 ms refresh queue; new global-state writes only refresh the catalog when a catalog setting changes.

## 5. Sorting and status

- The initial MCP consent dialog first highlights that the app icon appears only in the system tray, not on the taskbar, then explains MCP actions in the same dialog.
- By default, chats are ordered by activity area and received time; local click time is not treated as Codex activity.
- Name sorting, project filtering, date ranges, and archive views are available.
- Chat status can include unread, working, completed, failed, interrupted, and no-response diagnostics.
- Project and chat pin priority comes from Codex; local groups do not change Codex pins.
- Recent activity normally follows the selected day range. A chat may be set to always include or exclude through the context menu; the Activity rules dialog lists both overrides and can restore automatic behavior. Include uses a solid purple bar; exclude uses a striped purple bar.
- Default state priority is unread errors/waiting/unread/working/read. Read chats use newest-first conversation time; other states retain region-entry ordering. Acknowledged errors retain their current icon until recovery or a healthy next turn; new errors become unread again. Structured question answers and later user input resolve waiting; async questions survive turn completion. Recent status filters allow one selection and default to ALL. Project filtering remains multi-select. P controls recent project-column visibility, with separate project sorting and filtering. N cycles status/name ordering independently.
- Waiting uses `?` for both projects and chats. Projects and Chats each have independent close and upward restore controls, retaining their organization and saved expansion state.
- The app opens the Projects view, with the running version in the app-name tooltip. After flexible space are Back, Forward, a separator and Settings, followed by the existing window controls. Active chats archive through the context menu; archived chats retain a restore icon. Both actions verify state from catalog metadata without reading rollout bodies.
- Session-only navigation records successful opens through the existing Pane entry points. Consecutive duplicates are collapsed, new opens after Back replace the forward branch, and failed opens preserve the history position. Existing click/context-menu selection and linkage preferences are preserved; only arrow navigation actively selects its destination. Chat-title tooltips prefix the actual ID's last four characters; visible titles are unchanged.

## 6. Local persistence

The app separates settings from rebuildable analytics:

- `%CODEX_HOME%\.codex-chat-pane\settings.toml`: language, theme, font sizes, window mode and dimensions, preview switch, log level, and MCP consent.
- `folders.json` in the same directory: folders, groups, local project and chat assignments, stars, recent activity rules, order, and collapse state.
- `usage.sqlite` for quota samples and dismissals; identity-scoped `usage-cache-<digest>.sqlite` for own requests, reports, compact quota observations, append checkpoints, and short question-round mappings. Local estimates are not an account bill.

Without `CODEX_HOME`, the app uses `%USERPROFILE%\.codex`. Preview is off by default.

Filters, pane heights, and some frequently changing UI state are stored in browser `localStorage`. They are not synced to Codex.

## 7. Out of scope

- Uploading conversations, rollouts, logs, or local databases.
- Automatically converting local folders into Codex projects.
- Writing synthetic projects back to Codex.
- Promising compatibility with every future Codex Desktop version beyond its stable public APIs.
- Providing macOS or Linux window integration at present.
- Claiming exact live generation boundaries from completion-only logs. Σ uses no OpenTelemetry receiver, internal IPC, traffic proxy, additional app-server, account requests, or predicted TTFT.

The [feature-to-code map](product.zh-CN.md#8-功能文档与代码对应) connects all user, data, integration, performance, and delivery areas to their current documentation and implementation entry points. Historical benchmarks and current verification have separate labels.

The [documentation index](README.md) covers the full feature set and distinguishes request, technical turn, question round, conversation family and quota period. Feature descriptions use the delivery behavior; they are not claims of native interaction acceptance.
