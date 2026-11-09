# CodexChatPane

[English](README.md) | [简体中文](README.zh-CN.md)

CodexChatPane is a Windows sidebar for Codex Desktop that helps you find chats, follow progress, and check usage.

**[Download for Windows](https://github.com/tvivt/CodexChatPane/releases/latest)**

![CodexChatPane beside Codex Desktop; the sidebar is outlined in red](CodexChatPane.png)

## Find chats

- Find chats in Recent activity or under Projects and Chats, with search and sorting.
- Organize projects and chats in folders and groups; use stars, selection, and drag and drop to keep frequent work close at hand.
- Open or create a chat, or use its context menu to rename, pin, archive, or restore it. Chat previews can be enabled in settings.

## Follow progress

- See running, waiting-for-answer (`?`), unread, and error states, along with elapsed time and recent activity.
- Select a chat to view first-token latency, generation speed, and cache hit rate in the bottom status bar. Open Chat details from the metrics or context menu for turn summaries, activity counts, tools, and subagents.
- Use Σ to view the estimated total speed of currently running chats.

## Check usage

- Click the quota area at the bottom to open Reset history: the current period plus five recent periods, with ten more available at a time.
- Switch to Consumption to view Token usage and estimated API-equivalent cost by period, date, project, chat, or model.
- API-equivalent cost is an estimate, not your subscription bill. See [Usage and consumption](doc/usage-history.zh-CN.md) for definitions.

## v0.5.4

- Start each quota period at first use and label it by the reset that ended it.
- Show unconfirmed natural resets and estimated times clearly when observations are missing.
- Keep period selection, duration, Token usage, and cost totals aligned when late evidence arrives.

See the [v0.5.4 notes](doc/releases/v0.5.4.md). Windows downloads are available from [GitHub Releases](https://github.com/tvivt/CodexChatPane/releases).

## Compatibility and status

- Requires Windows, Codex Desktop, and WebView2 Runtime.
- The download is an unsigned portable executable.
- This is an experimental personal project, not an official Codex Desktop component. Codex updates may affect compatibility.

## Get started

1. Download the EXE from the [latest release](https://github.com/tvivt/CodexChatPane/releases/latest).
2. Open Codex Desktop, then run CodexChatPane. The sidebar reads your local projects and chats.
3. Choose a chat, organize the list, or open Settings to adjust the theme, language, font size, and window mode.

## Documentation

| Topic | Document |
| --- | --- |
| User guide and settings | [Usage](doc/usage.md) |
| Complete feature index | [Documentation](doc/README.md) |
| Latency, speed, and statistics definitions (Chinese) | [Conversation metrics](doc/conversation-metrics.zh-CN.md) |
| Quotas, consumption, and cost estimates (Chinese) | [Usage and consumption](doc/usage-history.zh-CN.md) |
| v0.5.4 changes | [Release notes](doc/releases/v0.5.4.md) |
| Source setup, tests, and builds | [Development and validation](doc/development.md) |

## Data and safety

- The app does not upload or host your conversations. Browsing does not modify the original Codex conversations.
- Folders, groups, themes, and window settings stay local.
- Renaming, pinning, and archiving change Codex state; enable write operations in settings before using them.

## License

[MIT License](LICENSE)
