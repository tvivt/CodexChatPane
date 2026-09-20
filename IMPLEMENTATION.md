# CodexChatPane 实现与接手指南

本文描述当前代码结构、数据流、缓存和验证。产品规则见 `SPEC.md`；Codex 协议见 `CODEX_INTEGRATION.md`。

## 1. 运行时结构

```text
Codex local files ──read-only──> Rust SourceSnapshot
                                      │ Tauri invoke
                                      ▼
config/* + localStorage ──merge─> frontend state ──> DOM

Chat preview icon ──invoke──> preview.rs ──paged rollout text
User action ──invoke──> Deep Link / codex_app_mcp.rs ──> Codex
```

Tauri 主窗口每秒调用 `get_snapshot`。Rust 扫描失败时 `lib.rs` 返回最后有效快照并附加错误；前端保留当前列表并显示同步失败。

## 2. 文件职责

| 文件 | 职责 |
| --- | --- |
| `src-tauri/src/lib.rs` | Tauri commands、窗口入口、Deep Link、ID 验证 |
| `src-tauri/src/source.rs` | 只读打开数据库和全局状态，合成 Snapshot |
| `src-tauri/src/source/rollout.rs` | 增量解析工作状态、执行时间、轮数、Token、额度和事件错误 |
| `src-tauri/src/source/diagnostics.rs` | 错误分类、桌面 retry 日志增量读取 |
| `src-tauri/src/preview.rs` | 对话正文按需分页和缓存 |
| `src-tauri/src/codex_app_mcp.rs` | 内部 App MCP 管道发现、调用、激活与验证 |
| `src/app.js` | 前端状态、Source 合并、渲染、排序和事件 |
| `src/dynamic.js` | Group、成员、折叠与稳定顺序存储 |
| `src/i18n.js` | 运行时中英文替换 |
| `src/styles.css` | 主题、布局与控件 |

## 3. Source 输入

Codex home：优先 `CODEX_HOME`，否则 `%USERPROFILE%\.codex`。

### `state_5.sqlite`

只读读取 `threads`。启动时严格检查以下字段：

```text
id created_at updated_at title name archived rollout_path
source thread_source cwd project_id
```

缺字段直接返回 Unsupported/错误，不猜 schema。

### `.codex-global-state.json`

读取：

- `local-projects` 与 `project-order`；
- `thread-project-assignments`；
- `projectless-thread-ids`；
- `pinned-thread-ids`；
- `electron-persisted-atom-state.unread-thread-ids-by-host-v1`。

### `thread_history_1.sqlite`

用于最后用户消息时间、当前 turn 活动、执行起止和状态。没有该库时允许退化为空活动映射。

### rollout JSONL

`source/rollout.rs` 只把正文降为“是否存在”，不保存或发送文本。它增量跟踪：

- user message 与 model activity；
- task/turn start、complete、interrupt、fail；
- pending tool call；
- token_count 的累计四类 Token 与 rate limits；
- compaction、错误和 queued message。

每个 rollout 有进程内 Cursor，文件追加时只读新增完整行；文件截断或替换时重建状态。

### Codex Desktop 日志

诊断读取 `%LOCALAPPDATA%\Codex\Logs` 当天/昨天日志中的 `codex_core::responses_retry`，只保留当前 active turn。App MCP 激活检测读取 Microsoft Store Codex 包目录中的窗口活动日志。

## 4. Snapshot 契约

`SourceSnapshot`：

```text
state error scopeKey hostId projects chats rateLimits
```

`ProjectSnapshot`：

```text
id name path count latest synthetic pathValid
```

`ChatSnapshot`：

```text
id projectId title createdAt updatedAt recencyAt archived
codexPinned codexUnread working lastUserMessageAt
executionStartedAt executionMs executionStatus activityAt
diagnostic turnCount tokenUsage
```

Rust 使用 `serde(rename_all = "camelCase")`；修改字段时同步检查前端 destructuring 与 fixtures。

## 5. Project 归类算法

对每个 Thread：

```text
projectless set
  > explicit thread-project assignment
  > threads.project_id
  > normalized cwd == normalized project path
  > synthetic:uncategorized
```

Project 总数与 latest 由最终 Chat 归属聚合。只有实际出现无项目 Chat 时才生成 synthetic Project。

## 6. 工作状态与时间

rollout 收到用户消息即建立 `working=true` 活动；后续 reasoning、assistant、工具调用或 compaction 更新响应时间。完成/失败/中断事件终止工作状态。

执行时长优先使用事件显式 `duration_ms`，否则由完成时间减开始时间。中断仍保存状态，但前端按产品规则隐藏时长。

前端 `timelineAt` 优先级：

```text
activityAt > lastUserMessageAt > updatedAt > createdAt
```

工具点击和本地排序不写回这些时间。

## 7. 诊断与额度

`Diagnostic.kind`：

```text
quota | network | promptRejected | retry | noResponse | other
```

`noResponse` 只有在工作中、已观察到用户消息、没有等待工具、120 秒没有模型活动时产生。任何后续有效活动清除 warning。

额度来自 rollout `token_count.rate_limits`，只接受 `limit_id` 为空或 `codex`。`primary` 为 5h，`secondary` 为 Week；`observedAt` 用于判断快照过期。前端把 used 转为 remaining。

## 8. 对话预览

`get_chat_preview(threadId, before?)` 先从只读 `state_5.sqlite` 查 rollout 路径。

- 每页 512 KiB；
- 最新页逐步扩大，最多 8 MiB，直到包含用户消息；
- 缓存键为 `threadId:end`，同时校验路径、长度、mtime；
- 最多 24 项，满后整体清空；
- 只输出 user、assistant、可见 tool/process 与 compaction；
- 跳过 reasoning 正文和 tool 输出正文；
- `before` 是上一页字节偏移，`currentUser` 是页内最后用户项索引。

这是唯一读取对话正文的后端路径。

## 9. 前端 Source 合并

`loadNativeSnapshot()` 防止并发扫描，也在 resize/drag 时跳过替换，避免布局跳动。

快照签名未变时只更新额度，不重渲染列表。变更时：

1. 用稳定 ID 查找上一份 Project/Chat；
2. Source 字段覆盖；
3. 恢复 Folder 关系；未归类项进入固定“项目/对话”文件夹；
4. 用互斥状态区和接收时间生成普通列表顺序；
5. 清理失效选择和项目过滤；
6. 保存 Folder 状态并渲染。

已读排序中的 `openedAt` 是当前应用会话状态，不写入 `folders-v1`；重启后回到 Source 时间排序。普通列表不使用手动顺序；拖动只改变 Folder/Group 归属。

## 10. 本地持久化

本地生成的 `config/settings.toml` 保存语言、主题族、亮暗模式、窗口层级、字号、窗口宽度、日志级别和内部 MCP 同意状态。关闭窗口固定隐藏到托盘。`config/folders.json` 保存稳定的 Project/Chat Folder 树、归属、星标、手动顺序和桌面端 Group，不写入 Source 时间或运行时区域。这两份本机状态不提交到仓库，首次运行时自动创建。原生数据加载完成前禁止把空前端状态回写到这些文件。

`preferences-v1` 保存分栏高度、筛选、打开状态等高频 UI 偏好；其中同名主题字段、`folders-v1` 和桌面端 `dynamic-v1` 只作迁移兼容。浏览器端 `dynamic-v1` 由 `dynamic.js` 管理 Group、成员和折叠；桌面端同一份数据随 `folders.json` 保存。

`dynamic.ensure()` 维护 Group 成员集合；呈现顺序仍由状态区与接收时间决定。自建 Group 的成员可复制到多个 Group。删除 Group 只删除本地关系。

配置导出为可读 TOML，由系统另存为对话框选择位置。文件只记录工具本地状态：外观/布局、自建文件夹树、项目/对话对自建文件夹的归属，以及非空的自建 Group。Codex Pin、归档、自动 PIN/项目/对话文件夹和未分组项都不写入，导入后由 Source 快照还原。`parse_exported_config` 接受 TOML 与旧 JSON；导入上限 5 MiB。

## 11. Tauri Commands

| Command | 说明 |
| --- | --- |
| `get_snapshot` | 后台线程执行 Source scan，失败保留最后快照 |
| `get_tool_config` / `save_tool_config` | 读取或写入稳定 TOML 设置 |
| `open_chat` | 打开 `codex://threads/<id>` |
| `new_chat` | 无项目 `threads/new`，有路径则 `threads/new?path=`；`ShellExecuteW` 打开，不用 Explorer |
| `run_codex_action` | rename/pin/archive |
| `get_chat_preview` | 分页预览 |
| `open_project_directory` | canonicalize 后交给 Explorer |
| `set_always_on_top` | 修改当前 Tauri 窗口 |
| `request_close` | 用户关闭主窗口时隐藏到托盘，并记录手动关闭状态 |
| `minimize_to_tray` | 自动隐藏到托盘；Codex 前台时允许恢复 |
| `export_config_file` | 另存为可读 TOML；取消时返回空 |
| `parse_exported_config` | 解析导出的 TOML 或旧 JSON |

所有 ID 和路径在 Rust 信任边界验证；文件路径先 canonicalize 并确认目录。

## 12. 启动与缓存

```powershell
npm install
npm run tauri dev
```

`npm install` 只在 lockfile 或 node_modules 缺失/变化时需要。不要把 `npm list` 放入每次启动流程；它会递归检查依赖树且不提供运行所需结果。

Vite 忽略 `src-tauri/**` 与 `.runtime/**`，前端修改由 HMR/页面刷新处理。Cargo 自行判断 Rust 增量编译；代码未变不应清理 `src-tauri/target`。

生产前端：

```powershell
npm run build
```

当前 `bundle.active=false`，仓库未配置安装包发布流程。

## 13. 验证矩阵

| 改动 | 最小检查 |
| --- | --- |
| CSS/HTML/前端渲染 | frontend test + `npm run build` |
| Group/排序 | `node tests/dynamic.mjs` + frontend test |
| Rust Source/preview/MCP | `cargo test` + `cargo fmt --check` |
| Tauri command 契约 | Rust 与 frontend test + build |
| 全面交付 | 下列全部命令 |

```powershell
npm run build
node tests/frontend.mjs
node tests/dynamic.mjs
cd src-tauri
cargo test
cargo fmt --check
```

另运行仓库根目录 `git diff --check`。不要为文档或单点逻辑变化做无关浏览器全面回归。

## 14. 已知脆弱点

- `state_5.sqlite` schema、全局 JSON 字段和 rollout 事件属于 Codex 本机实现。
- App MCP pipe 与 tool namespace 是内部动态能力，不能硬编码缓存。
- unread 集合可能缺失；此时保留 unknown，不把所有 Chat 猜成已读或未读。
- 纯 Vite 无原生数据，只能验证静态 UI 与前端逻辑。
- 原生 app 的性能瓶颈优先查重复 Source scan、日志 bootstrap 和误触发 Cargo/npm，而不是先加新缓存层。

## 15. 修改原则

- 数据归属和状态在 Rust Source 一次修正，不在多个 UI caller 打补丁。
- 固定 Group 由 Source 派生，自建 Group 由 `dynamic.js` 管理。
- 新 Codex 写操作必须包含：动态 `tools/list`、明确参数、错误返回、后置验证和一个测试。
- 新字段只有在当前 UI 或诊断需要时加入 Snapshot。
- 保持原生 JS/Tauri 结构；没有测量证据时不引入框架、状态库或数据库层。
