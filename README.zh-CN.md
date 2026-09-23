# CodexChatPane

[English](README.md) | [简体中文](README.zh-CN.md)

CodexChatPane 是一个运行在 Windows 上的 Codex Desktop 辅助侧栏：把本机 Codex 的 Project、Chat、运行状态和额度集中到一个可筛选、可分组的窗口中。

这是一个个人项目的实验性源码版本，不是 Codex Desktop 的官方组件，也不上传或托管对话数据。

## 功能概览

- 按 Project、Chat、最近活动和归档状态浏览本机 Codex 数据。
- 使用本地 Folder 和 Group 整理 Project、Chat；支持多级文件夹、搜索、排序、多选和拖动。
- 显示未读、执行中、错误、执行时间、最后活动时间和额度状态。
- 悬停 Chat 的预览入口，按需分页读取 rollout 内容。
- 通过 Deep Link 打开已有对话或新建对话。
- 在用户明确同意后，通过 Codex Desktop 内部 App MCP 修改对话名称、Pin、归档和 Project Pin。
- 提供主题、语言、字号、窗口模式、日志级别和工具配置导入/导出。

## 界面预览

![CodexChatPane 界面预览](CodexChatPane.png)

## 兼容性与当前状态

- Windows only。
- 需要可正常运行的 Codex Desktop；Codex 的本地数据结构和内部 MCP 可能随版本变化。
- 当前仓库发布源码；GitHub Actions 会构建未签名的 Windows 单文件可执行程序。
- 浏览器预览可以检查静态前端，但 Project、Chat、窗口联动和 Codex 操作需要桌面运行时。

## 快速开始

前置条件：Windows、Node.js/npm、Rust stable，以及 Codex Desktop。

```powershell
npm ci
npm run tauri dev
```

只启动前端预览：

```powershell
npm run dev
```

桌面运行时默认监听 `127.0.0.1:1420`。首次启动时，应用会读取本机 Codex 数据；本地工具设置和 Folder 状态写入 Windows 用户配置目录，默认是 `%APPDATA%\com.codexchatpane.app\`，不会写入仓库。

## 获取 Windows 可执行文件

推送到 `main` 或手动运行 GitHub Actions 的 `Windows portable build` 后，在工作流的 Artifacts 中下载 `CodexChatPane-windows-portable`，其中的 `CodexChatPane.exe` 可以直接运行。当前构建未签名，Windows 可能显示 SmartScreen 提示；程序依赖系统已安装 WebView2 Runtime。

从源码在 Windows 本地构建，在仓库根目录运行：

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\build-app.ps1
```

脚本执行与 CI 相同的安装、检查和构建命令，生成 `release\CodexChatPane.exe`。环境要求和详细步骤见 [开发与验证](doc/development.md)。

## 文档

| 目的 | 文档 |
| --- | --- |
| 怎么使用 | [doc/usage.md](doc/usage.md) |
| 理解产品行为和数据所有权 | [doc/product.md](doc/product.md) |
| 理解代码结构和数据流 | [doc/architecture.md](doc/architecture.md) |
| 理解 Codex 数据、Deep Link 和 App MCP | [doc/integration.md](doc/integration.md) |
| 修改、测试和构建项目 | [doc/development.md](doc/development.md) |

## 验证

```powershell
npm run build
node tests/frontend.mjs
node tests/dynamic.mjs
cargo fmt --manifest-path src-tauri/Cargo.toml --check
cargo test --manifest-path src-tauri/Cargo.toml --locked
```

Rust 测试中有少量显式忽略的本机 Codex 数据检查；它们需要真实 Codex 环境或对应环境变量，不属于默认回归。

## 数据与安全边界

- Codex 状态库、历史库、rollout 和日志以只读方式读取。
- 对话正文只在用户请求预览时按需读取，不写入本项目的快照或配置。
- Folder、Group、主题和窗口设置属于本地工具状态，不同步到云端。
- 改名、Pin、归档等写操作需要用户启用 Codex MCP，并在每次操作后重新加载状态验证结果。
- Codex Desktop 内部 App MCP 不是公开稳定接口；接口变化可能使写操作暂时不可用，但不影响只读浏览。

## License

[MIT License](LICENSE)
