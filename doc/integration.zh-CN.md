# Codex 接入说明

[English](integration.md) | [简体中文](integration.zh-CN.md)

CodexChatPane 依赖 Codex Desktop 的本机文件和部分桌面能力。它不是公开 Codex API 的替代品；内部接口变化时，读取和写入能力可能分别受到影响。

## 1. 本地数据

默认 Codex home 为：

```text
%USERPROFILE%\.codex
```

也可以通过 `CODEX_HOME` 指向其他目录。应用只读使用以下来源：

CodexChatPane 自身的设置单独保存在该 Codex home 下的 `.codex-chat-pane` 目录。

- `state_5.sqlite`：Thread 基础信息和 Project 关联。
- `.codex-global-state.json`：全局 Project、Thread assignment、Pin 和未读状态。
- `thread_history_1.sqlite`：按对话 ID 补充消息时间、缺失执行信息和失败状态；文件变化先比较最新轮次状态摘要，工具项写入不刷新全列表活动。
- rollout JSONL：增量提供工作状态、最近用户消息与进展时间、执行时间和错误；正文只在打开预览后按页读取。
- Codex Desktop 日志：有限的 retry 和活动诊断。

额度优先通过 Codex App MCP 读取，使用已有对话作为上下文。MCP 失败时，通过 Desktop CLI 的 app-server 查询 `account/rateLimits/read`。启动、窗口恢复可见时刷新额度，可见期间每分钟刷新一次；查询失败保留上次结果。

启动时会检查关键 SQLite 表和字段。schema 不满足要求时，应用报告不兼容，而不是猜测字段含义。

## 2. Deep Link

支持的主要路由：

```text
codex://threads/<thread-id>
codex://threads/new
codex://threads/new?path=<project-path>
```

新建 Project 对话使用项目路径，不使用已经废弃的 `projectId` 查询参数。所有 Thread ID 和路径都会先验证和编码。

## 3. Codex App MCP

管道发现优先使用 `codex-app-tools-*`，兼容旧版本的 `codex-browser-use-*`。新版本浏览器管道不再注册 App Tools；向它发送 `tools/list` 会返回 `No handler registered for method: tools/list`，不能据此判断所有 App 工具已移除。各候选管道仍通过工具目录确认所需能力，命名空间使用实际返回值。对话 Pin、Project Pin、重命名、归档/恢复和额度查询共用此发现逻辑。

本地只读协议核对：运行忽略的 `codex_app_mcp::tests::live_app_tool_catalog_matches_action_arguments` 检查实际工具名、必填字段及参数类型；`reads_live_usage_limits_over_mcp` 使用已有线程 ID 验证外部完整调用。这两项不更改真实对话。实际写操作仍需单独验证，目录匹配不能替代状态回读。

写操作默认关闭，需要用户在设置中明确启用。每次执行都会：

1. 重新发现可用的 Codex App MCP 工具。
2. 检查当前能力和参数映射。
3. 调用目标操作。
4. 等待 Codex 状态稳定。
5. 重新读取快照确认结果。

当前接入的写操作包括：

- Chat 重命名。
- Chat Pin / Unpin。
- Chat 归档或恢复。
- Project Pin / Unpin。

如果发现工具不可用、Renderer 未激活或 Codex 返回错误，界面显示失败提示，不把本地状态伪装成成功。

## 4. 安全边界

- 普通浏览不需要 MCP 同意。
- 数据库、历史库、rollout 和日志以只读方式打开。
- 应用不把对话正文上传到网络服务。
- Deep Link 和外部命令使用参数化路径，不拼接 shell 命令。
- App MCP 是 Codex Desktop 内部能力，不能视为稳定公共协议。

## 5. 兼容性排查

Codex 更新后如果出现空列表、schema 不兼容或写操作失败，按以下顺序检查：

1. 确认 `CODEX_HOME` 和 Codex Desktop 进程。
2. 检查 `state_5.sqlite` 是否可读以及关键字段是否存在。
3. 查看界面错误和本地日志级别。
4. 区分只读数据问题、rollout 解析问题和 App MCP 写操作问题。
5. 修改接入代码前先记录实际 schema 或工具返回，不凭名称推断。
