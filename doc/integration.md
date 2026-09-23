# Codex integration

[English](integration.md) | [简体中文](integration.zh-CN.md)

CodexChatPane uses local Codex Desktop files and some desktop capabilities. It is not a replacement for the public Codex API. Changes to internal interfaces may affect read and write features independently.

## 1. Local data

The default Codex home is:

```text
%USERPROFILE%\.codex
```

Set `CODEX_HOME` to use another directory. The app reads these sources without modifying them:

- `state_5.sqlite`: basic thread information and project associations.
- `.codex-global-state.json`: global projects, thread assignments, pins, and unread state.
- `thread_history_1.sqlite`: message times and activity state.
- Rollout JSONL: working state, execution time, tokens, usage limits, errors, and preview content.
- Codex Desktop logs: a limited set of retry and activity diagnostics.

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

Write operations are off by default and must be explicitly enabled in settings. Each operation:

1. Rediscovers available Codex App MCP tools.
2. Checks current capabilities and parameter mappings.
3. Calls the requested operation.
4. Waits for Codex state to settle.
5. Reloads the snapshot to verify the result.

Supported write operations include:

- Rename a chat.
- Pin or unpin a chat.
- Archive a chat.
- Pin or unpin a project.

If a tool is unavailable, the renderer is not active, or Codex returns an error, the UI reports failure instead of showing a false local success.

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
