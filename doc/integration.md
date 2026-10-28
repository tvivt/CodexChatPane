# Codex integration

[English](integration.md) | [简体中文](integration.zh-CN.md)

CodexChatPane uses local Codex Desktop files and some desktop capabilities. It is not a replacement for the public Codex API. Changes to internal interfaces may affect read and write features independently.

## 1. Local data

The default Codex home is:

```text
%USERPROFILE%\.codex
```

Set `CODEX_HOME` to use another directory. The app reads these sources without modifying them:

CodexChatPane's own settings are kept separately in `.codex-chat-pane` under that same Codex home.

- `state_5.sqlite`: basic thread information and project associations.
- `.codex-global-state.json`: global projects, thread assignments, pins, and unread state.
- `thread_history_1.sqlite`: supplements message times, missing execution details, and failure status by chat ID. File changes compare the latest-turn status summary; tool item writes do not refresh the whole list's activity.
- Rollout JSONL: incrementally supplies state, user/progress times, execution time and errors. Parsers temporarily inspect needed fields; complete content is shown on demand in paged previews. Catalog and statistics windows do not receive request/activity bodies; statistics receive summaries and paged turns.
- JSONL.ZST and history_base chains use the same logical reader. Previews may show inherited text; metrics/forecast ownership excludes inherited and child events. Migration preserves event times.
- Codex Desktop logs: a limited set of retry and activity diagnostics.

Usage limits are read through Codex App MCP first, using an existing chat as context. If MCP fails, the app queries `account/rateLimits/read` through the Desktop CLI app-server. Limits refresh at startup, when the window becomes visible, and once per minute while visible. A failed query keeps the last value.

New snapshots carry `observation_source=online-mcp/cli-fallback`; imported observations use `history-log`, while legacy provenance stays unknown. Existing quota storage retains mode/candidate checkpoints, redemption evidence and stable-event aliases. `startedAt/endedAt` are the adopted `[start,end)` boundary for both labels and consumption; `boundaryFrom/boundaryTo` retain observation evidence. The current interface provides no reliable reset-occurrence timestamp. First consumption cannot substitute for one.

Σ and speed statistics do not use an OpenTelemetry receiver, streaming internal IPC, WebView injection, traffic interception, additional app-server processes or account queries. Metadata rendering does not wait for quota or history bodies. Existing read-only quota fallback remains independent.

At startup, the app checks key SQLite tables and columns. If the schema does not match, it reports an incompatibility instead of guessing what fields mean.

## 2. Deep links

Main supported routes:

```text
codex://threads/<thread-id>
codex://threads/new
codex://threads/new?path=<project-path>
```

Creating a conversation in a project uses the project path, not the deprecated `projectId` query parameter. Thread IDs and paths are validated and encoded first.

## 3. Codex App MCP

Pipe discovery prefers `codex-app-tools-*` and retains `codex-browser-use-*` for older Desktop versions. Current browser pipes no longer register App Tools; calling `tools/list` on them returns `No handler registered for method: tools/list`. Each candidate is still checked for the required tool, using the namespace returned by its catalog. Chat pinning, project pinning, rename, archive/unarchive, and usage queries share this discovery path.

The ignored `codex_app_mcp::tests::live_app_tool_catalog_matches_action_arguments` test checks live tool names, required fields, and argument types without modifying chats. `reads_live_usage_limits_over_mcp` verifies an external end-to-end read using an existing thread ID. Catalog checks do not replace readback verification of actual write operations.

Write operations are off by default and must be explicitly enabled in settings. Each operation:

1. Rediscovers available Codex App MCP tools.
2. Checks current capabilities and parameter mappings.
3. Calls the requested operation.
4. Waits for Codex state to settle.
5. Reloads the snapshot to verify the result.

Supported write operations include:

- Rename a chat.
- Pin or unpin a chat.
- Archive or restore a chat.
- Pin or unpin a project.

If a tool is unavailable, the renderer is not active, or Codex returns an error, the UI reports failure instead of showing a false local success.

`codex_app_mcp.rs::run_action/run_project_pin` serialize the supported writes. A renderer-specific failure can activate the target through a deep link, await renderer evidence, retry once, and verify via state readback. This is not an arbitrary external tool proxy. Quota reads never redeem a reset card. Live catalog capability checks do not verify real writes.

## 4. Safety boundary

- Normal browsing does not require MCP consent.
- Databases, history, rollouts, and logs are opened read only.
- The app does not upload conversation text to a network service.
- Deep links and external commands use parameterized paths rather than concatenated shell commands.
- App MCP is an internal Codex Desktop capability, not a stable public protocol.

## 5. Compatibility troubleshooting

If lists become empty, the schema becomes incompatible, or write operations fail after a Codex update, check in this order:

1. Confirm `CODEX_HOME` and the Codex Desktop process.
2. Check whether `state_5.sqlite` is readable and has the required columns.
3. Review UI errors and the local log level.
4. Separate read-only data issues, rollout parsing issues, and App MCP write issues.
5. Record the actual schema or tool response before changing integration code; do not infer behavior from names alone.

## 6. Internal data test events

With the same build already running, launch a second process with `--internal-test-file=<JSON path>`. The single-instance callback forwards a JSON `{event,data}` to the main window; the file is at most 64 KiB and event names at most 64 ASCII letters/digits/`._-`. This accepts data, not JavaScript or arbitrary MCP operations. It is not dispatched on the initial process startup. Logs record handled/unhandled status.

The current event is `quota.reset-notice`, taking `{cycle,report}` with totals and a model rows array. The in-memory preview uses a fixed test ID, performs no consumption/quota query, and writes neither quota samples nor dismissal state. Dismissing the test clears only the preview; ordinary polling preserves it until dismissal or process exit. Real notification dismissals remain persistent. The [PowerShell example](integration.zh-CN.md#6-内部数据测试入口) uses BOM-free UTF-8 and shows the complete shape. This round did not execute the native preview manually; parser and in-memory rendering are covered by tests.
