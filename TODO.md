# TODO

## Mac 适配：保持窗口显示逻辑一致

状态：TODO，尚未实现。

目标：在 macOS 上支持 Codex Desktop 的本地数据、Deep Link、App MCP 和“随 Codex 显示”，使用前台应用事件控制显示/隐藏，不复制 Windows 的窗口层级方案。

### 任务

- [ ] 数据目录：`src-tauri/src/source.rs` 保留 `CODEX_HOME` 覆盖，默认补充 macOS 的 `HOME/.codex`；确认 `state_5.sqlite`、`thread_history_1.sqlite`、全局状态 JSON 和 rollout 文件结构。
- [ ] 路径匹配：`source.rs` 的 `normalized_path` 不再在 macOS 把 `/` 替换成 `\\` 或统一转小写；补充大小写敏感路径测试。
- [ ] 打开操作：`src-tauri/src/lib.rs` 将 `explorer.exe` 和 Windows Deep Link 分支分别适配为 macOS 的 `open` 命令；继续使用参数数组，不经过 shell 拼接。
- [ ] App MCP：`src-tauri/src/codex_app_mcp.rs` 把 PowerShell/WMI/Windows Named Pipe 发现拆成平台实现；在 Mac 实机确认 Codex App Tools 的实际 socket/pipe、环境变量和工具 namespace，再决定是否增加 Unix socket transport。
- [ ] 日志诊断：适配 macOS Codex Desktop 日志位置；如果 Mac 没有等价的 renderer 活动日志，改为明确降级而不是等待 Windows 路径。
- [ ] 窗口跟随：`src-tauri/src/window_attach.rs` 增加 `target_os = "macos"` 实现，用 `NSWorkspace` 的前台应用激活事件和 `frontmostApplication` 判断 Codex；`codex` 模式只在 Codex 前台时恢复自动最小化的 Pane，不因失去焦点主动隐藏，用户关闭后保持隐藏直到托盘恢复。不复制 Windows 的 HWND/窗口 owner 逻辑，也不恢复 100ms 轮询。
- [ ] 配置持久化：将 `src-tauri/src/lib.rs` 当前写入仓库 `config/` 的设置迁移到 Tauri 用户配置/数据目录，并保留旧文件的一次性迁移。
- [ ] 打包发布：`src-tauri/tauri.conf.json` 开启 bundle，增加 `.icns`，配置 macOS `.app`/DMG、Apple Silicon/Intel 构建、签名和公证。
- [ ] 验证文档：补充 Mac 实机测试和 README、实现文档中的平台说明。

### 窗口行为验收

- [ ] `normal`：窗口保持普通层级。
- [ ] `global`：窗口始终置顶。
- [ ] `codex`：Codex 成为前台应用时恢复自动最小化的 Pane；离开时不主动隐藏，切换只由前台应用事件唤醒。
- [ ] 启动时立即同步一次，重复前台事件不重复执行显示/隐藏操作。
- [ ] 用户点击关闭后不因 Codex 激活自动恢复，只能由托盘双击或右键“打开”恢复。
- [ ] 不要求 Accessibility 权限；只有 Tauri 显示/隐藏 API 不足时，才单独评估该权限。

### 暂不做

- 不重写前端 UI。
- 不假设 Mac 与 Windows 使用相同的 App MCP transport；先以 Mac 实机观测结果为准。
- 不把“取消置顶”扩大为“强制置于所有窗口底部”；两者语义不同。

## 对话错误类型细分

- [ ] reconnect / retry / 网络错误的状态与提示细分
- [ ] 额度耗尽与重置时间提示
- [ ] prompt rejected 与其他 Codex 错误分类
- [ ] 无响应判定与 Codex 原生错误结果的关联
