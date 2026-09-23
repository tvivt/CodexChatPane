# 架构说明

[English](architecture.md) | [简体中文](architecture.zh-CN.md)

CodexChatPane 是一个没有前端框架的 Tauri 2 桌面应用。Rust 负责本机数据读取、窗口和外部操作，原生 JavaScript 负责状态、渲染和交互。

## 1. 数据流

```text
Codex 本地文件
    │ 只读扫描
    ▼
Rust SourceSnapshot
    │ Tauri invoke
    ▼
前端 state ──> Project / Chat / Folder UI
    ▲                 │
    │                 ├─ Deep Link -> Codex Desktop
    │                 └─ App MCP  -> Codex 写操作
    │
CODEX_HOME/.codex-chat-pane + localStorage
```

## 2. 代码分层

### Rust / Tauri

- `src-tauri/src/lib.rs`：Tauri command、窗口入口、配置和 Deep Link。
- `src-tauri/src/source.rs`：读取 Codex 状态库并聚合 Project、Chat 快照。
- `src-tauri/src/source/rollout.rs`：增量解析 rollout 活动、执行时间、Token 和额度。
- `src-tauri/src/source/diagnostics.rs`：读取有限的桌面诊断日志并分类错误。
- `src-tauri/src/source_watch.rs`：监听 Codex 数据文件的目录和历史变化。
- `src-tauri/src/preview.rs`：按页读取对话预览，限制单页和缓存大小。
- `src-tauri/src/codex_app_mcp.rs`：发现、调用和验证 Codex Desktop 内部 App MCP。
- `src-tauri/src/window_attach.rs`：Windows 前台窗口识别和 Pane 显示联动。

### 前端

- `src/app.js`：应用状态、快照合并、排序、渲染和事件处理。
- `src/dynamic.js`：Group、成员、折叠和稳定顺序。
- `src/i18n.js`：运行时中英文文本。
- `src/styles.css`：布局、主题、窗口和控件样式。
- `index.html`：页面骨架和 SVG 图标库。

## 3. 启动与刷新

1. Tauri 启动主窗口和托盘。
2. 前端加载本地配置和兼容的 `localStorage` 状态。
3. Rust 扫描 Codex 状态库、全局状态和活动来源。
4. 前端用稳定 ID 合并快照，恢复本地 Folder 和 Group 关系。
5. 文件变化触发目录刷新；活动对话每秒查询一次状态，打开的预览另行按需读取。
6. Rust 扫描失败时保留最后有效快照，并向界面报告错误。

原生数据加载完成前不会把空的前端状态写回本地配置，避免启动竞态清空用户整理结果。

## 4. 快照边界

前端主要消费以下快照字段：

```text
state, error, scopeKey, hostId, projects, chats, rateLimits
```

Chat 的工作状态和诊断来自结构化事件与受限日志读取；rollout 正文不进入普通快照。修改字段时必须同时检查 Rust 的 `serde` 命名、前端读取和测试 fixtures。

## 5. 持久化边界

- 稳定工具配置写入 `%CODEX_HOME%\.codex-chat-pane\settings.toml`。
- Folder、Group、Star、动态规则和本地归属写入同目录下的 `folders.json`。未设置 `CODEX_HOME` 时根目录为 `%USERPROFILE%\.codex`。
- 高频界面状态写入浏览器 `localStorage`。
- Codex 原始数据库、rollout 和日志只读打开。
- 配置文件是本机运行状态，公开仓库不会提交它们。

## 6. 修改原则

- 先确认数据所有权，再决定写入哪一层。
- 新增快照字段时同时修改 Rust、前端和 fixture/test。
- 不在前端猜测 Codex schema；不确定时显示错误或降级。
- 共享排序、归类和状态逻辑保持单一来源。
- 跨 Rust/前端契约的修改必须运行两侧检查。
