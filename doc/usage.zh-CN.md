# 使用说明

[English](usage.md) | [简体中文](usage.zh-CN.md)

本文面向第一次使用 CodexChatPane 的用户。

## 1. 启动

开发环境在项目根目录执行：

```powershell
npm ci
npm run tauri dev
```

发布版直接运行 `CodexChatPane.exe`，无需安装；目标 Windows 需要已安装 WebView2 Runtime。

CodexChatPane 启动后会读取本机 Codex 数据。它不会创建新的 Codex 账户，也不会把数据上传到远端。

如果只执行 `npm run dev`，得到的是浏览器前端预览：可以查看布局和部分交互，但不能读取本机 Codex 数据、联动窗口或执行 Codex 操作。

## 2. 主界面

### Projects

Projects 区域用于从 Project 角度查看最近对话：

- 顶部动态区域列出最近 N 天的对话；“动态规则”按钮汇总手动加入或移出的对话。
- Project 结构区域显示 Codex Project 和本地 Folder。
- 选择 Project 后，右侧或下方区域显示该 Project 的 Chat。
- 没有 Project 的对话会进入固定的“项目/对话”归类，不会被伪造为真实 Project。

### Chats

Chats 区域用于全局浏览和整理对话：

- `PIN` 显示 Codex 中已 Pin 的对话。
- `ALL` 显示可用对话。
- 自建 Folder 可以多级嵌套；一个 Chat 可以被整理到本地 Folder。
- 支持搜索、Project 过滤、排序、归档视图和多选。

## 3. 常用操作

### 打开对话

点击 Chat 行或操作菜单即可让 Codex Desktop 打开对应对话。CodexChatPane 不复制对话正文，只通过 Deep Link 定位目标。

### 预览对话

对话预览默认关闭。在设置中开启后，Chat 行才显示预览入口。内容按需从对应 rollout 分页读取；关闭预览后不会把正文保存到工具配置。

若要覆盖最近天数过滤，右键对话选择“动态显示 → 自动、加入、移出”。“动态规则”对话框列出两组覆盖，也能把对话恢复为“自动”。

### 整理 Folder

在 Projects 或 Chats 区域创建本地 Folder，然后把 Project 或 Chat 拖入目标位置。Folder、Group、顺序和折叠状态属于 CodexChatPane，不会写回 Codex 的 Project 结构。

### 使用 Group

Group 是本地的交叉组织方式。一个 Project 或 Chat 可以属于多个 Group；删除 Group 只删除本地关系，不删除 Codex 数据。

### 修改 Codex 状态

重命名、Pin、归档和 Project Pin 属于 Codex 写操作：

1. 在设置中启用“Codex MCP 操作”。
2. 首次执行时确认授权。
3. 从 Chat 或 Project 菜单执行操作。
4. 等待操作完成；发生错误或报警时会显示常驻通知。

归档会要求再次确认；已归档 Chat 通常需要先恢复后才能继续重命名或 Pin。

## 4. 设置

设置页提供：

- 中文/英文。
- 亮色/暗色和主题族。
- Tab、栏和行字号。
- 普通窗口、随 Codex 显示、全局置顶。
- 日期颜色竖条。
- 对话预览开关（默认关闭）。
- 日志级别。
- Codex MCP 操作开关。
- 工具配置导入和导出。

本地设置位于 `%CODEX_HOME%\.codex-chat-pane\settings.toml`，Folder、Group、Star 和动态规则位于同目录的 `folders.json`。未设置 `CODEX_HOME` 时，根目录为 `%USERPROFILE%\.codex`。这些文件由应用生成，不进入仓库。

## 5. 常见问题

### 页面显示“正在读取 Codex 数据”或同步失败

确认 Codex Desktop 正在运行，并检查 Codex 数据目录是否可访问。可通过 `CODEX_HOME` 指向自定义数据目录。应用会保留最后一次有效快照，不会用空列表覆盖已有内容。

### 只能浏览，不能重命名或归档

只读浏览不依赖 MCP。写操作需要在设置中启用 Codex MCP；Codex 更新后内部 MCP 可能发生变化，此时写操作会失败，但不会影响本地 Folder 整理。

### 想重置本地界面状态

先关闭应用，再备份并移走 `%CODEX_HOME%\.codex-chat-pane\` 下的 `settings.toml` 和 `folders.json`。下次启动时会使用默认设置并重新创建本地配置。

### 想查看调试信息

在设置中将日志级别调为 `debug`。日志写入本机 CodexChatPane 数据目录，不提交到仓库。
