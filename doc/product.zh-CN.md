# 产品模型

[English](product.md) | [简体中文](product.zh-CN.md)

本文定义 CodexChatPane 当前版本的用户可见行为和数据边界。代码实现细节见 [architecture.zh-CN.md](architecture.zh-CN.md)，Codex 接入细节见 [integration.zh-CN.md](integration.zh-CN.md)。

## 1. 产品目标

CodexChatPane 解决的是本机 Codex Desktop 中 Project 和 Chat 较难同时浏览、筛选和整理的问题。它提供一个本地辅助视图，不替代 Codex Desktop，也不建立新的远端数据源。

## 2. 核心实体

| 实体 | 来源 | 含义 |
| --- | --- | --- |
| Project | Codex | Codex 中的项目身份、名称和路径 |
| Chat | Codex | 对话身份、标题、归档状态、所属 Project 和时间 |
| Folder | CodexChatPane | 本地的层级整理方式 |
| Group | CodexChatPane | 可交叉复用的本地集合 |
| Timeline | Codex 事件、rollout 与实时额度查询 | 最近活动、执行时间、当前状态和额度 |
| Archive | Codex | Codex 原生归档状态，不等同于本地 Folder 回收站 |

## 3. 数据所有权

- Codex 拥有 Project、Chat、归档、Codex Pin 和未读状态。
- CodexChatPane 拥有本地 Folder、Group、星标、动态规则、手动顺序、主题和窗口设置。
- 工作状态、执行时间、额度和诊断信息由 Codex 数据与事件推导，工具不改写原始事件。
- 日志解析可临时检查消息内容以提取状态和问答；完整正文只按需展示于预览/详情，不作为普通目录快照或工具配置保存。
- 精确统计按目标请求读取日志；轮次映射缓存只保留首次提问的短摘要。Σ 是短期预测，与已结算的用量、速度和费用分开，阶段状态不持久化复活。

## 4. Project 与 Chat 归类

Chat 的 Project 归类遵循以下优先级：

1. 明确的无 Project 标记。
2. Codex 全局的 Thread-Project assignment。
3. Thread 自身的 `project_id`。
4. Thread 当前工作目录与 Project 路径匹配。
5. 仍无法归类时进入合成的“无项目对话” Project。

合成 Project 只是界面占位，不写回 Codex。

## 5. 排序与状态

- 默认按活动区域和接收时间排序，不把本地点击时间当作 Codex 活动时间。
- 可切换名称排序、Project 过滤、日期范围和归档视图。
- Chat 状态可以包含未读、工作中、已完成、失败、被中断和无响应诊断。
- Project 和 Chat 的 Pin 优先级来自 Codex；本地 Group 不改变 Codex Pin。
- 最近动态通常由所选天数决定。右键可将对话设为“自动、加入、移出”；“动态规则”对话框汇总两组覆盖并可恢复自动。加入使用紫色实条，移出使用紫色斜纹条。
- 默认状态区域优先级为待回答、未读待查看、工作中、已读；进入区域的时间用于区域内顺序，目录刷新保留既有状态。结构化问题与普通正文问句不同，问题逐项回答或后续普通用户消息才使等待失效；异步问题不因 task_complete 自动消失。
- 待回答在 Project 和 Chat 行统一显示 `?`，文字放在悬停提示。Projects 的关闭与向上恢复按钮同时控制 Projects/Chats，Chats 不设独立按钮；恢复后保留此前组织结构。
- 主界面固定显示 Projects，标题加粗并显示包版本；独立 Chats 页没有标签入口。普通对话通过右键归档，归档对话保留恢复图标，均直接执行、不再二次确认；操作后通过目录元数据核验状态，不读取 rollout 正文。
- P 项目分组方向与 N 对话排序分别单选；N 在状态、名称升序、名称降序间互斥，状态没有反向模式。完整排序、拖动和右键动作见[使用说明](usage.zh-CN.md)。

## 6. 本地持久化

应用分别保存工具状态与可重建统计：

- `%CODEX_HOME%\.codex-chat-pane\settings.toml`：语言、主题、字号、窗口模式、窗口尺寸、预览开关、日志级别和 MCP 同意状态。
- 同目录下的 `folders.json`：Folder、Group、Project/Chat 的本地归属、星标、动态规则、顺序和折叠状态。
- `usage.sqlite`：在线额度快照、重置通知关闭标记；`usage-cache-<身份摘要>.sqlite`：自己的逐请求消费、报告、历史额度观察、用量检查点及问题轮次映射。来源/账号变化使用独立缓存，不能把本地历史估值称为账号账单。

未设置 `CODEX_HOME` 时使用 `%USERPROFILE%\.codex`；预览默认关闭。

筛选、分栏高度和部分高频界面状态保存在浏览器 `localStorage`。这些状态不会同步到 Codex。

## 7. 明确不做的事

- 不上传对话、rollout、日志或本机数据库。
- 不把本地 Folder 自动转换成 Codex Project。
- 不把合成 Project 写回 Codex。
- 不承诺兼容 Codex Desktop 的公开稳定 API 之外的所有未来版本。
- 当前不承诺 macOS 或 Linux 的窗口联动行为。
- 本地日志不能准确识别所有实时生成边界，不为 Σ 接入 OpenTelemetry、内部 IPC、流量代理、额外 app-server 或账户查询，不预测 TTFT 或未结算 Token 累计量。额度现有的只读 MCP/CLI 回退查询独立保留。

## 8. 功能文档与代码对应

| 领域 | 用户/口径文档 | 主要代码入口 |
| --- | --- | --- |
| Projects、Chats、动态/Folder/Group、菜单、排序、窗口与配置 | [使用说明](usage.zh-CN.md) | `app.js` 的 `renderProjectPane`、`itemMenu`、`handleMenuAction`、`restoreWindowGeometry`；`dynamic.js` |
| 状态和待回答 | [架构](architecture.zh-CN.md) | `source.rs`、`source/questions.rs`、`source/rollout.rs`、`chatRegion` |
| 请求速度、TTFT、Σ、继承与子代理 | [对话统计](conversation-metrics.zh-CN.md) | `metrics.rs`、`metrics/latest.rs`、`source/rollout/generation.rs` |
| 额度、重置、计价、问题轮次、模型趋势 | [额度与消耗](usage-history.zh-CN.md) | `usage.js`、`usage-preload.js`、`usage/costs.rs`、`usage/ledger.rs` |
| 本地数据、Deep Link、App MCP、测试事件 | [接入](integration.zh-CN.md) | `codex_app_mcp.rs`、`lib.rs`、`source_watch.rs` |
| 缓存、并发与启动边界 | [架构](architecture.zh-CN.md) | `load_snapshot`、`metrics/analysis.rs`、`usage/context.rs` |
| 运行、验证、构建、本机部署和公开发布 | [开发交接](development.zh-CN.md) | `scripts/build-app.ps1`、`tests/`、Tauri/Cargo/package 版本字段 |

完整功能清单和统计粒度辨析见[文档总览](README.md)。功能说明采用本批交付口径；它不代表所有原生交互均已人工验收。历史基准只证明对应固定样例，验证范围和限制分别在各专题及开发文档记录。
