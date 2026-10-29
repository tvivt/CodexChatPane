# 开发与验证

[English](development.md) | [简体中文](development.zh-CN.md)

目录：[环境](#1-开发环境) · [验证](#2-验证命令) · [构建与本机交付](#3-构建便携版-windows-exe) · [公开发布](#4-github-actions) · [修改入口](#5-修改检查表) · [提交边界](#6-提交边界) · [当前验证记录](#7-本次验证记录)。功能覆盖映射见[产品模型](product.zh-CN.md#8-功能文档与代码对应)。

## 1. 开发环境

- Windows
- Node.js/npm
- Rust stable / Cargo
- Codex Desktop（运行时数据验证需要）

安装依赖并启动开发窗口：

```powershell
npm ci
npm run tauri dev
```

仅检查前端：

```powershell
npm run dev
```

## 2. 验证命令

前端构建和无框架检查：

```powershell
npm run build
node tests/frontend.mjs
node tests/dynamic.mjs
```

Rust 格式和测试：

```powershell
cargo fmt --manifest-path src-tauri/Cargo.toml --check
cargo test --manifest-path src-tauri/Cargo.toml --locked
```

真实 Codex 数据/进程相关测试显式 ignore，不属于默认检查，数量随实现变化。不要无差别 `--include-ignored`：有些会查询额度或写 App 缓存，先读对应测试副作用。只读日志回放与临时隔离缓存的检查可按需单独运行：

```powershell
# 若复用本地构建脚本的依赖缓存，先设置这两个目录
$repoRoot = (git rev-parse --show-toplevel).Trim()
$env:CARGO_HOME = Join-Path $repoRoot '.runtime/cargo-home'
$env:CARGO_TARGET_DIR = Join-Path $repoRoot '.runtime/app-target'
cargo test --manifest-path src-tauri/Cargo.toml --lib --locked --offline --quiet
cargo test --manifest-path src-tauri/Cargo.toml --lib --locked --offline live_catalog_audit -- --ignored --nocapture
cargo test --manifest-path src-tauri/Cargo.toml --lib --locked --offline live_analysis_audit -- --ignored --nocapture
cargo test --manifest-path src-tauri/Cargo.toml --lib --locked --offline live_generation_replay -- --ignored --nocapture
```

`live_catalog_audit` 验证首目录不读 rollout，后台状态时间单独报告。`live_analysis_audit` 在临时 App 数据库中对照旧完整读取和新检查点；测试含三个本机固定样例，不是可移植 CI fixture，换机器应先确认 ID 是否存在。`CODEX_PANE_ROUND_VERIFY_IDS` 可为问题轮次对账指定逗号分隔的线程 ID；`CODEX_PANE_VERIFY_METRICS_THREAD` 用于 `local_latest_request_matches_full_history`。这些值只放本地环境，不提交账号/日志。普通回归应使用仓库合成数据。格式检查或构建通过均不能替代原生窗口交互验收。

## 3. 构建便携版 Windows exe

在仓库根目录运行（首次安装或依赖变更时需要可用的 npm 包源；Rust 依赖须已缓存）：

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\build-app.ps1
```

本地脚本按内容指纹判断是否需要 `npm ci` 和前端构建，未变化时复用 `node_modules` 与 `dist`；依赖缺失或前端产物变化也会触发重建。指纹保存在 `.runtime/build-cache/`。每次仍运行 Cargo 的变更检查，使用 `--release --locked --offline` 并设置 `CARGO_INCREMENTAL=1`。首次切换增量模式需要建立缓存，后续复用；独立 EXE 的资源或 Rust 代码变化仍需要编译/链接。

最终输出为 `.runtime/bin/CodexChatPane.exe`，Cargo 中间输出保留在 `.runtime/app-target/`，依赖缓存位于 `.runtime/cargo-home/`。相同 EXE 不重复复制；更新正在运行的 EXE 前需先退出应用。EXE 不包含 WebView2 Runtime 安装器，目标机器需要已有 WebView2 Runtime。

开发仓库的脚本不自动执行上面的完整验证命令；发布前单独运行需要的检查。该仓库构建脚本的独立回归检查为 `powershell -NoProfile -File tests/build-app.ps1`，使用临时目录与模拟 npm/Cargo 验证缓存失效和失败恢复，不会重装项目依赖。公开 release 仓库保留在线构建脚本，执行验证后输出 `release/CodexChatPane-v<version>.exe`，供 GitHub Actions 使用。

### 本机安装约定与重启核验

通用输出是 `.runtime/bin/CodexChatPane.exe`；本机交付约定覆盖 `D:\AI\CodexChatPane\CodexChatPane.exe`，这个目录不是项目的通用默认值。部署前实时查询 `Get-Process` 的 Path 并以 `Win32_Process.ExecutablePath` 交叉核对，退出**正确安装路径**进程并确认消失，再运行 build-app、复制、启动。不要沿用先前 PID，也不要仅因两个旧 EXE 哈希相等就宣称新源码已部署。

成功交付须记录：构建成功、构建文件与安装文件 SHA256 相同、新文件时间、新 PID、可执行路径、StartTime、Responding=True。`Get-FileHash -Algorithm SHA256`、`Get-Process -Id <新PID>` 和 CIM 查询可完成核验。运行版本不变时，哈希和进程证据比版本字符串更能区分新旧构建。脚本不会替你复制到安装目录或重启；不要修改原始 Codex 数据、认证或个人配置来“验证”部署。

## 4. GitHub Actions

开发检出不含发布工作流；公开发布检出保留 `Windows portable build`，只由 `v*` 标签或手动触发。与包版本匹配的标签会发布 `CodexChatPane-v<version>.exe`，手动运行只上传 `CodexChatPane-windows-portable` artifact。在 `doc/releases/` 维护对应版本说明，CI 成功后同步到 GitHub Release。本地构建、备份提交与公开发布是独立步骤；更新功能或文档不自动提交、推送、改版本或发版。

开发仓库是文档的唯一维护处。公开的 `README.md`、`README.zh-CN.md`、`doc/` 和截图同步到发布仓库；内部设计文档只保留在开发仓库。发布前在开发仓库更新一次，再同步公开子集。

版本字段位于 `package.json`、`src-tauri/Cargo.toml` 和 `src-tauri/tauri.conf.json`，本次文档核对版本为 0.5.2；修改版本时同步锁文件中的本包版本。发布任务才同步改版本和附件名，固定本地入口保留 CodexChatPane.exe。文档的历史 v0.2.1 记录只描述当时产物。

## 5. 修改检查表

- 行为修改：更新 `doc/product.zh-CN.md` 和对应前端/Rust 检查。
- 数据源或快照修改：同时检查 `src-tauri/src/source*`、`src/app.js` 和测试 fixtures。
- Deep Link 或 MCP 修改：更新 `doc/integration.zh-CN.md`，并验证成功、失败和不可用分支。
- 布局或交互修改：更新 `doc/usage.zh-CN.md`，运行前端检查；需要视觉验收时使用当前版本的脱敏数据。
- 配置修改：不要把 `%CODEX_HOME%\.codex-chat-pane\` 下的个人状态加入提交。
- 速度/归属修改：先读[统计交接](conversation-metrics.zh-CN.md)；`summary` 单次/轮次实测、`recentSpeed` 预测样本、`generation` 阶段是不同接口，禁止用预测回填真实用量或首字。
- 冷启动/缓存修改：先读[架构](architecture.zh-CN.md)的启动和失效表；验证元数据首屏、旧状态保持、后台补齐与 source/account 竞态，避免为了显示而全量扫日志。
- 额度/费用/轮次修改：先读[额度说明](usage-history.zh-CN.md)；维护 `indexed` 的完整覆盖、JSONL 半尾行、旧累计基线、主/子/继承去重；对照完整读取器与问题轮次合计，未知保持未知。
- 菜单/测试协议修改：`itemMenu`、`contextmenu`、`handleMenuAction` 与[菜单速查](usage.zh-CN.md#6-右键菜单速查)一起核对；内部通知测试入口见[接入文档](integration.zh-CN.md#6-内部数据测试入口)。
- 文档修改：先核对[全功能索引](README.md)，同步中英文 README、使用、产品、架构、接入与开发说明，以及受影响的统计专题。检查相对链接、章节锚点和废弃口径；功能完成说明、历史测试记录、当前运行验证分别表达。仅改文档不需要重编译、重启或再次扫描真实日志。

## 6. 提交边界

公开仓库应只提交源码、测试、锁文件、必要文档和许可证。不要提交：

- `node_modules/`
- `dist/`
- `src-tauri/target/`
- `release/`
- `.runtime/`
- `.codegraph/`
- `src-tauri/gen/`
- `%CODEX_HOME%\.codex-chat-pane\` 下的个人 `settings.toml` 和 `folders.json`
- 本机日志、对话导出和真实截图

## 7. 本次验证记录

v0.5.2 在 2026-10-29 的同步发布检出中通过：默认 Rust 136 passed、25 ignored，前端检查、动态列表检查、Rust 格式、前端生产构建、6 处版本字段一致性、152 个文档相对文件链接和 `git diff --check`。300ms 延迟退出回归先复现旧测试的 200ms 超时报错，采用程序实际的 2 秒退出等待后，完整默认测试通过。未运行原生界面交互和真实 MCP 写操作；公开附件由标签工作流构建。

v0.5.1 的远端 CI 在 `stalled_queries_and_process_exits_are_bounded_and_retryable` 中失败：正常响应后的 PowerShell 未在测试专用的 200ms 内退出，因而没有生成公开附件；v0.5.2 修复该测试。下面的 v0.5.1 通过记录仅指当时本地发布检出。

v0.5.1 在 2026-10-29 的同步发布检出中通过：默认 Rust 136 passed、25 ignored，前端检查、动态列表检查、Rust 格式、前端生产构建、6 处版本字段一致性、152 个文档相对文件链接和 `git diff --check`。本机 EXE 从同一发布检出构建；GitHub 标签工作流单独构建公开附件，并直接使用对应版本说明作为 Release 正文。未运行原生界面交互及真实归档/恢复写操作。

v0.5.0 在 2026-10-29 的同步发布检出中通过：默认 Rust 136 passed、24 ignored，前端检查、动态列表检查、Rust 格式、前端生产构建、版本与锁文件一致性、文档相对文件链接和 `git diff --check`。可下载 EXE 由标签工作流另行构建；自动检查不替代原生窗口交互验收。

以下保留此前执行记录中的 2026-10-28 本机 **Debug 后端**和无浏览器前端检查，源日志持续增长，数字只对应当时快照。本版未重跑这些真实数据测量，不是 Release 原生首屏计时或全面验收，也不证明后来调整的 Σ 口径已经过真实数据验收。

- 当时默认 Rust：101 passed、20 ignored；前端 `tests/frontend.mjs` 和 `tests/dynamic.mjs` 通过。回归包含缓存先显示、延迟变化队列在切分组/筛选/周期时不丢失、在途旧结果、关闭恢复及账号隔离。旧 Σ 阶段策略的验证不沿用为当前验收；当前应按统计文档验证工具等待保留、无静默淘汰、压缩边界、未知/部分覆盖、3 请求加权和配置隔离。
- 目录 578 个主对话：34/25 ms，rollout opens=0、bytes=0；后台补状态分别 3831/2094 ms，读源内容约 245.6/1.21 MB。后者受活动日志及进程状态影响，不能把 25 ms 当整个应用就绪。
- 长续接 / 中断 / 旧压缩三样例共 2507/2062/15 条自身请求，新旧请求字段、summary、消费相同。旧完整解析 1226/834/11 ms，新首次 1238/1058/16 ms，缓存再次 91/100/4 ms，三者源内容 bytes=0。首次分析并非全部更快。
- 最近两天问题轮次＋未识别与消费请求分别 375/258/15 条相等；主样例 5 段（3 段压缩）。最新有效单次请求与完整详情对账通过。未识别仍可出现，不推造归属。
- `live_generation_replay` 只读三个样例；记录了 1705/1480/13 次外层等待阶段，possible 证据间隔大于 30 秒为 41/81/0 次，最长约 263 秒。确认算法需保留未知状态；它不提供生成真值或实时精度保证。

未运行本轮浏览器/原生窗口视觉与完整交互验收、真实 MCP 改名/Pin/归档、所有账号切换和大文件原地替换组合、完整历史性能/内存压测、Release 原生启动性能。压缩预览扩大窗口仍可能重复解码；创建时间/小块校验不等于任意篡改的全文件验证。安装文件与进程证据以本轮交付报告为准。
