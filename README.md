# CodexChatPane

Windows 上的 Codex Desktop 辅助侧栏。它从本机 Codex 数据中聚合 Project、Chat、运行状态、额度与诊断信息，并通过 Deep Link 或 Codex App MCP 打开或修改对话。

当前版本为个人使用的实验性 Tauri 2 应用，仓库版本 `0.1.0`。

## 接手入口

按任务读取文档，不要把所有文档同时塞入上下文：

| 任务 | 文档 |
| --- | --- |
| 理解产品行为、排序和数据所有权 | [SPEC.md](SPEC.md) |
| 修改布局、样式或交互 | [FRONTEND.md](FRONTEND.md) |
| 修改代码、数据源、测试或启动流程 | [IMPLEMENTATION.md](IMPLEMENTATION.md) |
| 修改 Deep Link、MCP 或 Codex 操作 | [CODEX_INTEGRATION.md](CODEX_INTEGRATION.md) |
| 查看早期视觉方向 | `PROTOTYPE-A.html`、`prototype-a-reference.png` |

代码和运行时是最终事实来源。文档描述当前实现，不是未来路线图。

## 当前能力

- Projects：最近 N 天对话、Project 文件夹和当前 Project 的 Chat 文件夹三段浏览；默认“项目/对话”文件夹承接未归类项。
- Chats：全局 PIN、ALL 及自建多级文件夹；支持项目过滤、搜索、多选和拖动归类。
- Chat 状态：未读、工作中、已读、错误、最后活动时间和执行时间。
- 对话预览：按需读取 rollout，默认定位到最后一次用户输入，可向上翻页。
- Codex 操作：打开、新建、重命名、Pin/Unpin、归档/恢复。
- 状态栏：最后发送时间、执行时间、5h/Week 剩余额度与重置时间。
- 界面：设置页管理语言、主题、字号、关闭行为和 Codex MCP；标题栏保留置顶与窗口操作。

## 技术栈

- Tauri 2 + Rust 2021
- 原生 JavaScript、HTML、CSS
- Vite 7
- SQLite 只读访问：`rusqlite` bundled
- 无前端框架，无运行时 npm 依赖

## 目录

```text
index.html                    正式页面与 SVG 图标库
src/app.js                    前端状态、渲染和交互
src/dynamic.js                Group、成员、折叠与稳定顺序存储
src/i18n.js                   中英文运行时翻译
src/styles.css                布局、主题和控件样式
src-tauri/src/lib.rs          Tauri 命令与窗口入口
src-tauri/src/source.rs       Codex 数据聚合
src-tauri/src/source/         rollout 与诊断增量读取
src-tauri/src/preview.rs      对话预览分页读取
src-tauri/src/codex_app_mcp.rs Codex App MCP 客户端
tests/                        无框架前端与动态分组检查
PROTOTYPE-A.html              保留的原型，不参与构建
```

## 本地运行

前置条件：Windows、Node.js/npm、Rust stable、可正常运行的 Codex Desktop。

```powershell
npm install
npm run tauri dev
```

仅检查前端：

```powershell
npm run dev
```

Vite 固定监听 `127.0.0.1:1420`。Tauri/Cargo 和 Vite 都使用自身增量缓存；代码未变化时不应主动清理或全量重建。

## 验证

```powershell
npm run build
node tests/frontend.mjs
node tests/dynamic.mjs
cd src-tauri
cargo test
cargo fmt --check
```

Rust 测试中有三个显式忽略的本机数据检查，需要对应环境变量或真实 Codex 数据，不属于默认回归。

## 数据与安全边界

- Codex 状态库、历史库、rollout 和日志只读打开。
- 语言、主题、窗口置顶、字号、窗口宽度、关闭行为和内部 MCP 同意状态写入本地生成的 `config/settings.toml`；稳定 Folder 树、归属、星标、手动顺序和桌面端 Group 写入 `config/folders.json`。这两份本机状态不提交到仓库，首次运行时自动创建。分栏高度、筛选、打开状态等高频 UI 状态保存在 `localStorage`；浏览器端以 `localStorage` 作为全部本地数据的后备存储。
- 对话内容仅在用户悬停预览图标时按需读取；普通快照不保留正文。
- 改名、Pin 和归档是显式写操作，首次使用前需要用户同意内部 MCP。
- 内部 App MCP 不是公开稳定接口；每次操作都重新发现工具并验证结果。
- 指定 Project 新建对话使用 `codex://threads/new?path=<绝对路径>`；无项目使用 `codex://threads/new`。

## 当前限制

- 仅实现本机 Windows Codex Desktop。
- Codex 数据表、全局状态 JSON、日志和内部 MCP 都可能随 Codex 更新变化。
- 已归档对话在界面中只允许恢复；先恢复后才能改名或 Pin。
- 工具本地文件夹和 Group 不会写回 Codex。

## 接手检查

1. 先运行完整验证，确认问题不是已有脏工作区造成。
2. 数据归类问题从 `source.rs` 的 Project 归属优先级开始查。
3. 工作状态、执行时间、额度或错误问题从 `source/rollout.rs` 与 `source/diagnostics.rs` 开始查。
4. 列表顺序、Group 或拖动问题从 `app.js` 的排序函数和 `dynamic.js` 开始查。
5. Codex 操作问题按 `CODEX_INTEGRATION.md` 的发现、调用、激活、验证四段排查。
6. 修改后至少运行与改动对应的最小检查；跨 Rust/前端契约时运行全部检查。
