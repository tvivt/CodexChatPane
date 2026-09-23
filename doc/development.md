# 开发与验证

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

在仓库根目录运行（需联网安装依赖）：

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\build-app.ps1
```

脚本依次运行 `npm ci`、两项前端检查、`cargo fmt --check`、`cargo test --locked`、前端构建和发布版 Cargo 构建；CI 直接运行同一脚本。CI 使用 Node.js 22 和 Rust stable；本地使用相同版本可进一步对齐环境。

最终输出为 `release/CodexChatPane.exe`，Cargo 的中间输出保留在 `src-tauri/target/release/`。EXE 不包含 WebView2 Runtime 安装器，目标机器需要已有 WebView2 Runtime。

## 4. GitHub Actions

发布仓库的 `Windows portable build` 工作流在 `windows-latest` 上执行前端检查、Rust 格式和测试，然后上传 `CodexChatPane.exe` artifact。推送到 `main` 或手动运行工作流都可以触发构建。

## 5. 修改检查表

- 行为修改：更新 `doc/product.md` 和对应前端/Rust 检查。
- 数据源或快照修改：同时检查 `src-tauri/src/source*`、`src/app.js` 和测试 fixtures。
- Deep Link 或 MCP 修改：更新 `doc/integration.md`，并验证成功、失败和不可用分支。
- 布局或交互修改：更新 `doc/usage.md`，运行前端检查；需要视觉验收时使用当前版本的脱敏数据。
- 配置修改：确认本机 `config/` 文件仍被忽略，不把个人状态加入提交。

## 6. 提交边界

公开仓库应只提交源码、测试、锁文件、必要文档和许可证。不要提交：

- `node_modules/`
- `dist/`
- `src-tauri/target/`
- `.runtime/`
- `.codegraph/`
- `src-tauri/gen/`
- `config/settings.toml`
- `config/folders.json`
- 本机日志、对话导出和真实截图
