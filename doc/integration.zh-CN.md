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
- rollout JSONL：增量提供工作状态、最近用户消息与进展时间、执行时间和错误；解析时临时检查必要字段，完整正文仅按需进入分页预览，不进入普通目录或统计窗口；统计窗口只传摘要和分页轮次。
- `.jsonl.zst` 和 `history_base`：统一读取压缩及续接链，预览可以包含继承正文，计量/预测按目标 thread_id 排除继承；迁移不改事件时间。
- Codex Desktop 日志：有限的 retry 和活动诊断。

额度优先通过 Codex App MCP 读取，使用已有对话作为上下文。MCP 失败时，通过 Desktop CLI 的 app-server 查询 `account/rateLimits/read`。启动、窗口恢复可见时刷新额度，可见期间每分钟刷新一次；查询失败保留上次结果。

新额度快照附 `observation_source=online-mcp/cli-fallback`，历史合并附 `history-log`；旧来源缺失不猜测。识别器区分窗口、事件和消耗周期，持久保存模式、候选、核销证据与稳定事件别名，不增加采集通道。`startedAt/endedAt` 是显示与消费共同采用的半开区间，`boundaryFrom/boundaryTo` 保留原观察时间，仅供诊断。接口目前没有可靠的重置发生时间字段，不能用首次消费时间代替。

速度统计不接入 OpenTelemetry 接收器、用于实时流式事件的内部 IPC、流量代理或 WebView 注入。Σ 完全依赖本地日志和既有最新请求缓存，不增加 app-server 或账号请求。元数据首屏也不等待额度查询或历史正文。

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

代码入口为 `codex_app_mcp.rs::run_action/run_project_pin`；写操作串行、能力按需发现，不支持任意外部工具透传。仅在返回明确 renderer 未激活错误时，Deep Link 激活目标、等待渲染日志并重试一次，最后回读 Codex 状态核验。管道目录能列出工具不等于真实写操作已成功。额度读取使用更短超时，失败再走现有 CLI 回退并清理查询进程；不会兑换重置卡。界面同意开关与外部 API 的身份/能力边界不同。

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

## 6. 内部数据测试入口

`lib.rs::read_internal_test_event` 接受第二个实例的 `--internal-test-file=<JSON 文件>`；应先运行同一构建的 App，再启动第二个短进程，由 single-instance 回调把数据转给主窗口 `internal-test-event`。文件最多 64 KiB；event 名最多 64 个 ASCII 字母、数字或 `._-`；只接受 JSON 数据，不执行脚本，也不转发任意 MCP 操作。处理状态以 `internal test event name=... handled=...` 记入 App 日志。第一次启动的参数不作为测试事件分发通道。

当前前端注册的事件只有 `quota.reset-notice`，数据为 `{cycle, report}`：cycle 使用重置周期字段，report 至少含 totals 和 rows 数组。`createQuotaNoticeView.preview` 使用固定测试 ID、强制 W/提前重置/未关闭，仅在内存展示，不调用额度/消费索引，不写 quota_samples 或 quota_ack；点击测试通知的关闭按钮只移除预览。普通轮询不覆盖测试预览，退出应用后消失。真实通知关闭仍走持久化 ack。

仓库根目录中的示例（只在明确需要查看通知时手动运行）：

```powershell
$paneExe = (Resolve-Path '.runtime/bin/CodexChatPane.exe').Path
$testNow = [DateTimeOffset]::Now.ToUnixTimeMilliseconds()
$testFile = Join-Path (Get-Location) '.runtime/test-quota-notice.json'
$totals = @{input=100;cached=90;output=10;reasoning=5;requests=1;conversations=1;costUsd=0.01;unpricedRequests=0}
$testEvent = @{event='quota.reset-notice';data=@{
  cycle=@{startedAt=$testNow-86400000;observedStart=$testNow-86400000;endedAt=$testNow;boundaryFrom=$testNow;remaining=60}
  report=@{totals=$totals;rows=@(@{key='test-model';label='Test model';totals=$totals})}
}}
$testJson = $testEvent | ConvertTo-Json -Depth 8
[IO.File]::WriteAllText($testFile, $testJson, [Text.UTF8Encoding]::new($false))
Start-Process -FilePath $paneExe -ArgumentList ('--internal-test-file="{0}"' -f $testFile) -WindowStyle Hidden
```

示例用无 BOM 的 UTF-8，兼容 Windows PowerShell 5/PowerShell 7。示例数据与真实账单无关，不将测试通知当作后端额度验证。本轮未通过原生 UI 执行该测试入口；数据校验和内存预览由单元/前端检查覆盖。
