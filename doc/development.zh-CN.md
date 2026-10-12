# 开发与验证

[English](development.md) | [简体中文](development.zh-CN.md)

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

三个本机 Codex 数据测试默认忽略；它们需要真实数据或 `CODEX_PANE_VERIFY_THREAD` 等环境条件，不应作为普通 CI 的硬依赖。

## 3. 构建便携版 Windows exe

在仓库根目录运行（首次安装或依赖变更时需要可用的 npm 包源；Rust 依赖须已缓存）：

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\build-app.ps1
```

本地脚本按内容指纹判断是否需要 `npm ci` 和前端构建，未变化时复用 `node_modules` 与 `dist`；依赖缺失或前端产物变化也会触发重建。指纹保存在 `.runtime/build-cache/`。每次仍运行 Cargo 的变更检查，使用 `--release --locked --offline` 并设置 `CARGO_INCREMENTAL=1`。首次切换增量模式需要建立缓存，后续复用；独立 EXE 的资源或 Rust 代码变化仍需要编译/链接。

最终输出为 `.runtime/bin/CodexChatPane.exe`，Cargo 中间输出保留在 `.runtime/app-target/`，依赖缓存位于 `.runtime/cargo-home/`。相同 EXE 不重复复制；更新正在运行的 EXE 前需先退出应用。EXE 不包含 WebView2 Runtime 安装器，目标机器需要已有 WebView2 Runtime。

开发仓库的脚本不自动执行上面的完整验证命令；发布前单独运行需要的检查。该仓库构建脚本的独立回归检查为 `powershell -NoProfile -File tests/build-app.ps1`，使用临时目录与模拟 npm/Cargo 验证缓存失效和失败恢复，不会重装项目依赖。公开 release 仓库保留在线构建脚本，执行验证后输出 `release/CodexChatPane.exe`，供 GitHub Actions 使用。

## 4. GitHub Actions

发布仓库的 `Windows portable build` 工作流在 `windows-latest` 上运行同一脚本，并上传 `CodexChatPane-windows-portable` artifact。推送 `v*` 标签或手动运行工作流会触发构建；标签构建成功后还会创建 GitHub Release。

开发仓库是文档的唯一维护处。公开的 `README.md`、`README.zh-CN.md`、`doc/` 和截图同步到发布仓库；内部设计文档只保留在开发仓库。发布前在开发仓库更新一次，再同步公开子集。

## 5. 修改检查表

- 行为修改：更新 `doc/product.zh-CN.md` 和对应前端/Rust 检查。
- 数据源或快照修改：同时检查 `src-tauri/src/source*`、`src/app.js` 和测试 fixtures。
- Deep Link 或 MCP 修改：更新 `doc/integration.zh-CN.md`，并验证成功、失败和不可用分支。
- 布局或交互修改：更新 `doc/usage.zh-CN.md`，运行前端检查；需要视觉验收时使用当前版本的脱敏数据。
- 配置修改：不要把 `%CODEX_HOME%\.codex-chat-pane\` 下的个人状态加入提交。

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
