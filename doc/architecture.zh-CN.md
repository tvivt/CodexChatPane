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
- `src-tauri/src/source/rollout.rs`：增量解析 rollout 活动和执行时间。
- `src-tauri/src/source/rate_limits.rs`：通过 App MCP 读取实时额度，失败后走 CLI app-server，限制查询时间并清理查询进程。
- `src-tauri/src/source/diagnostics.rs`：读取有限的桌面诊断日志并分类错误。
- `src-tauri/src/source_watch.rs`：Codex home 根目录不递归监听，只递归监听 `sessions` / `archived_sessions`。
- `src-tauri/src/preview.rs`：按页读取对话预览，限制单页和缓存大小。
- `src-tauri/src/metrics.rs` / `metrics/latest.rs`：完整自身详情与最新请求增量读取；后者另外维护至多 3 条预测样本，保持单次实测 summary 不变。
- `metrics/details.rs`：完整计量后释放请求/活动正文，仅缓存轮次及全量汇总；4 个线程槽、50 条轮次/页，快照 ID 保持翻页一致，账号/来源/认证变化隔离。冷读或版本变化后的重开仍可能完整解析。
- `src-tauri/src/source/rollout/generation.rs`：复用活动游标提取本线程轮次和模型配置，输出 `generation`；运行/终止沿用活动状态，不另造一套生命周期或读取任务。
- `src-tauri/src/metrics/analysis.rs`：自己的请求、旧累计基线、问答边界和额度观察的一次流式分析及持久追加检查点。
- `src-tauri/src/usage/{context,costs,ledger}.rs`：身份隔离、消费索引/计价与额度周期；`costs/rounds.rs`、`ledger/logs.rs` 分别处理问题轮次和按月额度补读。
- `usage/ledger/recognizer.rs`：只接收紧凑额度观察，统一固定/浮动模式、候选、稳定事件与采用边界；检查点及旧通知别名存既有额度库，独立于 Token 索引。
- `src-tauri/src/codex_app_mcp.rs`：发现、调用和验证 Codex Desktop 内部 App MCP。
- `src-tauri/src/window_attach.rs`：Windows 前台窗口识别和 Pane 显示联动。

### 前端

- `src/app.js`：应用状态、快照合并、排序、渲染和事件处理。
- `src/dynamic.js`：Group、成员、折叠和稳定顺序。
- `src/usage.js` / `usage-preload.js`：额度独立窗口、通知、缓存先显示与按真实消费者合并变化 ID；无启动全周期预加载。
- `src/i18n.js`：运行时中英文文本。
- `src/styles.css`：布局、主题、窗口和控件样式。
- `index.html`：页面骨架和 SVG 图标库。

## 3. 启动与刷新

1. Tauri 启动主窗口和托盘。
2. 前端加载本地配置和兼容的 `localStorage` 状态。
3. `get_snapshot` 与 `get_catalog_snapshot` 都先走 `scan_catalog`：只读状态库和全局目录，不读 rollout、历史消息、诊断、项目路径文件系统或额度。缓存中的执行/待回答状态保留，冷态未知不假装完成。
4. 前端立即用稳定 ID 合并并渲染，恢复 Folder/Group，随后通过 `get_chat_activity` 异步补历史状态和项目路径检查。首次批量投影后，只读运行中、待回答、选中或实际变化的目标；片段投影优先，缺失时回退逻辑线程 ID。`activityPending` 保留新线程或换路径目标。
5. 目录变化只更新列表元数据并复用活动缓存；rollout 变化只更新受影响对话 ID。历史库变化比较缓存的最新轮次状态摘要，只补查实际变化的 ID，包括晚到的失败详情；工具输出和投影变化不重读所有 rollout。
   活动对话每秒增量检查 rollout 和诊断，不查询历史库；耗时使用缓存开始时间和本地时钟。打开的预览另行按目标对话分页读取，隐藏或关闭后停止刷新。
   窗口隐藏时保留待更新对象，恢复可见时集中处理，不全量重扫执行历史。额度在启动、恢复可见时独立刷新，可见期间每分钟刷新。监听溢出或重新连接时允许全量扫描，补回丢失的事件。
6. Rust 扫描失败时保留最后有效快照，并向界面报告错误。

原生数据加载完成前不会把空的前端状态写回本地配置，避免启动竞态清空用户整理结果。

快照锁只用于克隆/比较版本和短暂提交，历史查询、路径验证和解压在锁外。提交前核对目录身份与版本，竞态时重试或保留有效快照。`latest` 按线程独立锁并合并同目标读取，慢压缩线程不持有全局 latest I/O 锁。消费索引仍串行更新其数据库，前端不替其他窗口取消已启动的共享任务。

## 4. 快照边界

前端主要消费以下快照字段：

```text
state, error, projects, chats, activityPending
project: pathChecked, pathValid, error
chat: activitySynced, turnId, metricsRevision, generation
generation: turnId, phase, at, config {model, effort, serviceTier}
```

Chat 的工作状态和诊断来自结构化事件与受限日志读取；rollout 正文不进入普通快照。修改字段时必须同时检查 Rust 的 `serde` 命名、前端读取和测试 fixtures。

`generation` 提供当前轮次与配置证据，不是服务端解码状态的确认信号。`runningMetricsEntries` 复用既有运行状态与 `recentSpeed`，对同轮、同配置的有效预测求和；工具等待或静默不触发过期。有可靠当前压缩状态时排除，历史 `compacted` 完成记录及 `.zst` 文件格式不代表正在压缩。缺少开始信号时不能保证覆盖整个压缩区间。绘制 tick 只使用内存，不发起历史读取。精确定义见[对话统计](conversation-metrics.zh-CN.md)。

## 5. 持久化边界

- 稳定工具配置写入 `%CODEX_HOME%\.codex-chat-pane\settings.toml`。
- `remoteDebuggingPort` 默认 `0`（关闭）。“设置 / 诊断”接受 `0–65535`；非零端口在完全退出并重启工具后开启仅本机连接的 WebView2 CDP。主窗口和预览共用启动时参数，并保留原有浏览器默认参数；保存新端口不改变当前运行的 WebView 环境。
- Folder、Group、Star、动态规则和本地归属写入同目录下的 `folders.json`。未设置 `CODEX_HOME` 时根目录为 `%USERPROFILE%\.codex`。
- 高频界面状态写入浏览器 `localStorage`。
- Codex 原始数据库、rollout 和日志只读打开。
- 配置文件是本机运行状态，公开仓库不会提交它们。
- 额度/消费 SQLite 保存在 App 自己的目录，详情及轮次短摘要可能持久缓存；完整正文和预测阶段不写配置。见[额度存储](usage-history.zh-CN.md#存储与性能)。

### 缓存与失效

| 缓存 | 内容/边界 | 失效与资源界限 |
| --- | --- | --- |
| `metrics/latest.rs` | 当前轮次 parser、最新有效请求、至多 3 个同配置样本 | 32 个线程槽；追加读完整尾行，截断/创建身份/压缩/依赖变化重建；无预测阶段持久化 |
| `metrics/details.rs` | 轮次摘要、完整汇总及工具/子代理元数据 | 4 个线程槽；窗口每页 50 轮；翻页零正文读取，快照失效重新打开；不缓存或传输请求/活动明细 |
| `metrics/analysis.rs` | 验证后的用量、legacy 基线、问答短摘要和紧凑额度观察 | 持久版本 `usage-analysis-v1`；普通 JSONL 安全偏移追加，检查路径/创建时间/长度/mtime、首行及游标前小块；替换或历史链变化重建 |
| `preview.rs` / `rollout_log.rs` | 分页正文、链元数据、压缩逻辑长度 | 预览 24、链 32、长度 128 项，逐项淘汰；不缓存整份解压正文；扩大压缩预览窗口仍可能重复顺序解码 |
| `usage/costs.rs` | 逐请求明细、索引覆盖区间、按 UTC 日修订的报告 | `details-v7`；标题/项目/父子/覆盖/价表及相关日期变化失效；部分读取不认证完整范围 |
| `usage/ledger.rs` | 当前身份下按输入版本构建的额度周期 | 8 项；新快照/导入修订/覆盖月/关闭标记/身份变化失效；分钟通知走 live-only，不合并导入历史 |
| `usage/ledger/recognizer.rs` | 窗口模式、候选、事件证据、输入前缀与关闭别名 | `quota_derivations` / `quota_event_alias` 持久化；追加续判，历史插入仅重放紧凑观察；同一采用边界驱动显示与缓存分组 |

追加检查是常规日志写入协议的优化，不是对任意同元数据、旧中段原地篡改的全文件校验；无完整内容哈希或文件系统日志证明。缓存命中可免读源正文，首次大日志/压缩读取仍可能较慢，不能把目录延迟等同整个 App 就绪时间。

## 6. 修改原则

- 先确认数据所有权，再决定写入哪一层。
- 新增快照字段时同时修改 Rust、前端和 fixture/test。
- 不在前端猜测 Codex schema；不确定时显示错误或降级。
- 共享排序、归类和状态逻辑保持单一来源。
- 跨 Rust/前端契约的修改必须运行两侧检查。
