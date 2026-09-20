# Codex Desktop 交互能力

本文是 CodexChatPane 与 Codex Desktop 交互的单一事实来源。最后按本机安装版与当前代码核对：2026-09-06。

## 1. 三种通道

| 通道 | 用途 | 是否写 Codex | 稳定性 |
| --- | --- | --- | --- |
| 本地数据读取 | Project、Chat、状态、额度、诊断、预览 | 否 | 依赖本机文件结构与 schema |
| `codex://` Deep Link | 打开或创建对话、让 Codex 获得焦点 | 创建时会 | 已确认路由可用，参数仍可能演进 |
| Codex App MCP | 改名、Pin、归档等结构化操作 | 是 | 内部接口，运行时动态发现 |

快捷键模拟不在当前链路中。它存在焦点竞争、按键串扰和界面延迟问题，不作为回退。

## 2. Deep Link

### 2.1 已确认路由

| URI | 行为 | 当前使用 |
| --- | --- | --- |
| `codex://threads/<threadId>` | 打开并跳转到指定对话 | 是 |
| `codex://threads/new` | 新建无项目对话 | 是 |
| `codex://threads/new?path=<encoded-absolute-path>` | 在指定本地工作区新建对话 | 是 |
| `codex://new?projectId=<projectId>` | 无效；`codex://new` 需要 `prompt`/`path`/`originUrl`，且会被资源管理器当成文件夹 | 否 |
| `codex://threads/new?prompt=<encodedPrompt>` | 新建并预填提示词，不保证自动发送 | 否 |
| `codex://shared-thread/<shareId>` | 打开共享快照 | 否 |
| `codex://settings/...` | 打开 Codex 设置区域 | 否，具体子路由需再次验证 |

指定项目时只传工作区绝对路径 `path`。`projectId` 不是新建对话 Deep Link 的合法参数。

### 2.2 当前调用

前端：

```js
await invoke('new_chat', {
  projectId: project.synthetic ? null : project.id,
  projectPath: project.synthetic || !project.pathValid ? null : project.path,
});
await invoke('open_chat', { threadId: chat.id });
```

Rust 只按路径构造 URI，再用 `ShellExecuteW` 交给已注册的 Codex 协议处理器。不要用 `explorer.exe` 或未加引号的 `cmd start` 打开带 `?` 的 Deep Link，两者都会被 Windows 当成文件夹：

```rust
let uri = match project_path.filter(|path| !path.is_empty()) {
    Some(path) => format!("codex://threads/new?path={}", encode(path)),
    None => "codex://threads/new".to_string(),
};
open_uri(&uri)?; // ShellExecuteW("open", uri)
```

打开项目文件夹走单独的 `open_project_directory`，canonicalize 后才交给 `explorer.exe`。

`threadId` 与 `projectId` 在信任边界验证为不超过 128 字符，且仅包含 ASCII 字母、数字、`-`、`_`。

### 2.3 已排除路径

`codex://new?projectId=` 与 `codex://threads/new?projectId=` 都不是当前安装版的合法新建参数。指定 Project 必须传该项目的绝对路径 `path`。

Deep Link 不提供已确认的改名、Pin、归档、恢复或移动项目能力；这些操作走 MCP。

## 3. 当前接入的 App MCP 写操作

| 工具 | 参数 | CodexChatPane 动作 |
| --- | --- | --- |
| `set_thread_title` | `{threadId, title}` | `rename` |
| `move_thread_to_sidebar_section` | `{threadId, sectionId:"pinned"}` | Pin |
| `move_thread_to_sidebar_section` | `{threadId, sectionId:null}` | Unpin |
| `set_thread_archived` | `{threadId, archived:true}` | 归档 |
| `set_thread_archived` | `{threadId, archived:false}` | 恢复 |

`move_thread_to_sidebar_section` 是通用 Section 操作；传特殊 Section `pinned` 才表示 Pin。当前没有独立 `set_thread_pinned` 调用。

标题先 `trim`，不能为空，最多 256 个 Unicode 字符。

### 3.1 前端调用

```js
await invoke('run_codex_action', {
  threadId: chat.id,
  action: 'rename', // rename | pin | archive
  title: 'New title',
  enabled: null,
  settleDelayMs: 1000,
});
```

Pin 与归档通过 `enabled` 表示目标状态。调用结束后前端强制刷新 Source 快照。

### 3.2 工具参数映射

```json
{"threadId":"<id>","title":"<title>"}
{"threadId":"<id>","sectionId":"pinned"}
{"threadId":"<id>","sectionId":null}
{"threadId":"<id>","archived":true}
{"threadId":"<id>","archived":false}
```

## 4. 内部 MCP 传输与调用

### 4.1 发现 Named Pipe

1. 每次写操作查询 Windows `codex.exe` 进程命令行。
2. 从 `CODEX_APP_TOOLS_PIPE_PATH` 相关内容提取 `codex-browser-use-*`。
3. 连接 `\\.\pipe\codex-browser-use-*`。
4. 最长等待 20 秒，每 400 ms 重试一次。

不缓存管道地址。Codex 重启后地址可能变化。

### 4.2 帧格式

管道帧为：

```text
4-byte little-endian payload length
UTF-8 JSON payload
```

请求与响应最大 8 MiB。请求使用 JSON-RPC 2.0 结构：

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "tools/list",
  "params": {"threadStartKind": "all"}
}
```

### 4.3 动态能力检查

每次操作先调用 `tools/list`，按工具 `name` 查找当次返回的 `namespace`。不存在时立即报错，不使用硬编码 namespace，也不在程序启动时缓存能力。

### 4.4 调用工具

```json
{
  "jsonrpc": "2.0",
  "id": 2,
  "method": "tools/call",
  "params": {
    "arguments": {"threadId":"<id>","archived":true},
    "callId": "codex-chat-pane-<timestamp>-<counter>",
    "namespace": "<tools/list 返回值>",
    "threadId": "<id>",
    "tool": "set_thread_archived",
    "turnId": "codex-chat-pane-turn-<timestamp>-<counter>"
  }
}
```

成功条件是结果中的 `success === true`。失败时优先显示 `contentItems[].text`。

### 4.5 Renderer 未激活回退

写操作可能因 Codex renderer 未聚焦返回 `-32000` 或包含 `renderer` 的错误。当前处理：

1. 记录最近 Codex 日志文件偏移。
2. 打开 `codex://threads/<threadId>`。
3. 等待目标对话出现 `active=true`、窗口 `focused=true`、`visible=true` 的新增日志。
4. 再等待调用传入的稳定时间，限制在 1–5 秒。
5. 重新发现管道并重试一次。

没有截图、坐标点击或快捷键参与该流程。

### 4.6 后置验证

MCP 返回成功后，最多 3 秒、每 250 ms 重新扫描本地 Source：

- rename：标题等于目标标题；
- pin：`codexPinned` 等于目标状态；
- archive：`archived` 等于目标状态。

超时返回“工具成功但状态尚未更新”，不会假装成功。

## 5. Codex App MCP 能力清单

以下是当前 Codex App 环境暴露的能力分类，不代表 CodexChatPane 已接入。调用前仍必须以当次 `tools/list` 为准。

### 5.1 Chat 与任务

| 工具 | 主要参数 | 能力 |
| --- | --- | --- |
| `list_threads` | `{limit?}` | 列出非归档任务、Chat、Pin 与 Sidebar Section |
| `list_archived_threads` | `{cursor?, hostId?, limit?}` | 分页列出单主机归档任务 |
| `read_thread` | `{threadId, hostId?, cursor?, turnLimit?, includeOutputs?, maxOutputCharsPerItem?}` | 读取状态、轮次和可选输出 |
| `create_thread` | `{prompt, target, title?, model?, thinking?}` | 创建 project/worktree、projectless 或 ChatGPT Work cloud 任务 |
| `fork_thread` | `{threadId?, environment?}` | 分叉当前或指定任务；环境为 same-directory/worktree |
| `send_message_to_thread` | `{threadId, prompt, hostId?, model?, thinking?}` | 向现有任务发送用户可见消息 |
| `wait_threads` | `{targets:[{threadId,hostId?,afterCursor?}], timeoutMs?}` | 等待最多八个任务完成或需要处理 |
| `set_thread_title` | `{threadId?, title}` | 重命名任务 |
| `set_thread_archived` | `{threadId?, hostId?, archived}` | 归档或恢复任务 |
| `share_thread` | `{threadId?, hostId?}` | 创建不可变分享链接 |

`create_thread.target` 当前支持：

```text
{type:"project", projectId, environment:{type:"local"}}
{type:"project", projectId, environment:{type:"worktree", startingState?}}
{type:"projectless", directoryName?}
{type:"chatgptWorkCloud", projectId?}
```

`startingState` 可为 `{type:"working-tree"}` 或 `{type:"branch",branchName,onMissing?}`。只有用户明确要求新建该分支时才使用 `onMissing:"create-branch"`。

### 5.2 Project、Sidebar 与导航

| 工具 | 主要参数 | 能力 |
| --- | --- | --- |
| `list_projects` | `{}` | 列出本机、远程和 ChatGPT Project，含 Git 标志 |
| `move_thread_to_sidebar_section` | `{threadId, sectionId, hostId?}` | 移到 `pinned`、自建 Section 或普通列表 |
| `move_project_to_sidebar_section` | `{projectId, sectionId}` | 移动或 Pin Project |
| `create_sidebar_section` | `{name}` | 创建自建 Section |
| `rename_sidebar_section` | `{sectionId, name}` | 重命名 Section |
| `delete_sidebar_section` | `{sectionId}` | 删除 Section，不删除其中任务/Project |
| `reorder_section` | `{sectionId, threadIds}` | 重排 pinned 或自建 Section 中全部任务 |
| `reorder_sidebar_sections` | `{sectionIds}` | 重排全部自建 Section |
| `reorder_sidebar_projects` | `{projectIds}` | 重排默认 Projects 区中的未 Pin Project |
| `navigate_to_codex_page` | `{threadId}` | 让主窗口显示指定任务/Chat |
| `open_in_codex` | `{target, placement?, threadId?}` | 打开文件、Browser、Terminal 或 Review Tab |

`open_in_codex.target` 形态：

```text
{type:"file",path,line?}
{type:"browser",url?,tabId?}
{type:"terminal",sessionId?}
{type:"review",view?,path?,baseBranch?}
```

### 5.3 执行位置与协调

| 工具 | 主要参数 | 能力 |
| --- | --- | --- |
| `handoff_thread` | `{threadId, destinationHostId?, followUpPrompt?}` | 在 checkout/worktree 或匹配主机间交接 |
| `get_handoff_status` | `{operationId, afterRevision?, waitMs?}` | 等待交接状态变化 |
| `load_workspace_dependencies` | `{}` | 读取内置 Node/Python/文档库路径 |
| `read_thread_terminal` | `{}` | 读取当前桌面任务终端输出 |

### 5.4 账户与应用

| 工具 | 主要参数 | 能力/约束 |
| --- | --- | --- |
| `get_usage_limits` | `{}` | 读取账户各额度窗口与重置时间 |
| `consume_usage_reset` | `{idempotencyKey}` | 用户明确授权后兑换已有重置额度 |
| `automation_update` | `{mode,...}` | 创建/更新/查看/删除 heartbeat 或 cron 自动化 |
| `capture_screen_context` | `{}` | 仅活动语音会话读取当前 Codex 页面 |
| `end_realtime_voice_call` | `{}` | 仅用户明确要求时结束语音会话 |
| `uninstall_plugin` | `{plugin}` | 用户明确要求时卸载本地 Codex plugin |

`automation_update` 的创建核心字段：heartbeat 使用 `{kind:"heartbeat",name,prompt,rrule,status,...}`；cron 使用 `{kind:"cron",name,prompt,rrule,status,executionEnvironment:"local",projectId,model,reasoningEffort,...}`。更新和删除必须先解析已有 automation ID。

CodexChatPane 当前没有直接调用本节大多数工具。新增接入时先证明 Deep Link 或现有 Source 无法满足，再扩展 `action_request`，并为参数映射与后置状态留下一个可运行测试。

## 6. UI 能力边界

- 普通对话：允许打开、改名、Pin/Unpin、归档。
- 归档对话：允许打开或恢复；改名与 Pin 显示禁用说明。恢复后再执行。
- 工具 Pin 已删除，界面 Pin 完全复用 Codex pinned 状态。
- 工具本地 Group 的移动和复制在桌面端写入 `config/folders.json`、浏览器端写入 `localStorage`；Folder 归属写入同一持久层；二者都不调用 Codex。
- “无项目对话”新建使用裸 `codex://threads/new`。

## 7. 失败分类

| 阶段 | 典型错误 | 处理 |
| --- | --- | --- |
| Deep Link | Windows 无法打开协议 | 显示打开失败 |
| 管道发现 | 未找到 Codex App Tools pipe | 提醒启动 Codex Desktop |
| 能力发现 | `tools/list` 无所需工具 | 报告当前 Codex 未提供该工具 |
| Renderer | `-32000` / renderer unavailable | Deep Link 激活后重试一次 |
| 调用 | `success !== true` | 显示 MCP 返回文本 |
| 验证 | 3 秒内 Source 未变化 | 报告状态尚未更新 |

## 8. 相关实现

- `src-tauri/src/lib.rs`：Tauri 命令、Deep Link 构造与 ID 验证。
- `src-tauri/src/codex_app_mcp.rs`：管道发现、协议、工具调用、激活和验证。
- `src-tauri/src/source.rs`：写操作后的事实验证来源。
- `src/app.js`：用户同意、动作可用性与调用入口。
