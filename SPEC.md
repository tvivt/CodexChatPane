# CodexChatPane 当前产品规格

本文只定义当前产品行为与不变量。实现位置见 `IMPLEMENTATION.md`，视觉细节见 `FRONTEND.md`，Codex 调用见 `CODEX_INTEGRATION.md`。

## 1. 产品目标

CodexChatPane 是 Codex Desktop 的本机导航与状态侧栏：稳定查找 Project/Chat，同时观察未读、工作中、错误和最近完成状态，并用工具本地 Folder/Group 组织内容。

非目标：替代 Codex 对话界面、编辑对话正文、同步多设备布局、写入 Codex SQLite、通过快捷键自动化 Codex UI。

## 2. 核心实体

### Project

来自 Codex 全局状态，身份为稳定 `projectId`。字段包括名称、主路径、路径有效性、Chat 总数和最后活动时间。

`synthetic:uncategorized` 是工具生成的“无项目对话”，仅在存在无项目 Chat 时出现。它使用紫色 Project 图标，没有文件系统路径。

### Chat

身份为 Codex `threadId`。Source 字段包括所属 Project、标题、时间、归档、Codex Pin/unread、工作状态、执行状态、诊断、轮数和四类累计 Token。

### Folder

工具本地树形容器。Project Folder 组织 Project；Chat Folder 组织当前 Project 的 Chat。支持多级、折叠、重命名、拖动和本地回收。它们不写回 Codex。

### Group

Chats 全局视图中的集合：

- `PIN`：派生自 Codex Pin，只能由 Pin 操作改变。
- `ALL`：全部未归档 Chat，包含已 Pin Chat。
- `N Days`：Projects Tab 顶部的最近活动快捷视图，默认 3，可调 1–365。
- 自建 Group：可多级嵌套、复制/移动成员和调整 Group 顺序。

固定 Group 是派生视图，不保存成员；同一 Chat 可以同时出现在 PIN、ALL 和自建 Group。

### Timeline 与 Archive

Timeline 严格按接收到的最后活动时间倒序，不复用普通列表状态区排序。当前实现只保留全局 Timeline；Project 下栏只显示 Archive。

归档以 Codex Source `archived` 为准。工具本地回收 Folder 是另一种隐藏原因，不伪造 Codex 归档。

## 3. 数据所有权

| 数据 | 所有者 | 工具行为 |
| --- | --- | --- |
| Project/Chat 身份、名称、归档、Pin、unread | Codex | 读取；授权后通过 MCP 修改部分字段 |
| 工作状态、执行时间、额度、错误 | Codex 事件与日志 | 只读推导 |
| Folder 树、归属、星标与手动顺序 | CodexChatPane | 桌面端 `config/folders.json`；浏览器端 `localStorage` |
| Group、成员关系、各视图顺序与折叠状态 | CodexChatPane | 桌面端 `config/folders.json`；浏览器端 `localStorage` |
| 主题、语言、窗口置顶、字号、窗口宽度 | CodexChatPane | 本地 `config/settings.toml` |
| 关闭行为与内部 MCP 同意状态 | CodexChatPane | 本地 `config/settings.toml` |
| 分栏高度、布局、打开状态与筛选偏好 | CodexChatPane | `localStorage` |
| 对话正文预览 | Codex rollout | 悬停时只读分页 |

Source 每秒扫描一次。Codex 字段覆盖旧快照，本地 Folder/Group/布局继续保留。

## 4. Project 归属

Chat 归属优先级：

1. 位于 `projectless-thread-ids`：强制进入“无项目对话”。
2. `.codex-global-state.json` 的 `thread-project-assignments`。
3. `state_5.sqlite.threads.project_id`。
4. Chat `cwd` 与 Project 主路径规范化后精确匹配。
5. 仍无法匹配：进入“无项目对话”。

不得只按 `cwd` 猜测；显式 Project ID 和 projectless 标记优先。

## 5. 页面结构

### 标题栏与状态栏

自定义标题栏包含应用名、设置、窗口置顶和标准窗口操作。语言、主题、字号、关闭行为、Codex MCP 和配置导入/导出在设置页。

底部状态栏中部显示悬停或选中 Chat 的最后发送时间与 `[执行时间]`；右侧显示可用的 5h/Week 剩余额度、彩色环和重置时间；同步失败显示在左侧。

### Projects Tab

默认 Tab。依次为最近 N 天 Chat、Projects 文件夹、当前 Project 的 Chats 文件夹三段，分隔处可调整高度。

Projects 支持搜索、新建本地 Folder、多选、拖动和右键操作。Project 行显示未读/工作状态、名称以及未读、工作中、总数。

Project Chats 支持项目内搜索、新建 Folder、新建当前 Project Chat，以及打开当前 Project Archive 下栏。

### Chats Tab

全局 Chat 视图。每行使用“项目列 + 时间色条 + 预览图标 + 标题 + 归档/Pin 操作”。项目列宽按当前最长名称计算，上限 20 个显示字符。

Chats 工具栏支持项目多选过滤、Project 排序、搜索、新建 Group 和新建 Chat。

下方可切换全局 Timeline 或全部归档 Chat，二者由右下角按钮打开。

## 6. 排序

普通 Chat 状态区域互斥，优先级固定：未读、工作中、已读。各区域按接收到的最后消息时间倒序；本次刚被 Codex 标记为已读时，以当前工具会话记录的 `openedAt` 进入已读区顶部。

显式名称排序开启时，在状态区域内按名称升/降序。Project 排序开启时，在状态区域内按 Project 升/降序，并在不同 Project 间加分隔线。

PIN 与 ALL 是独立一级固定 Group。ALL 始终包含 Pin Chat；PIN 是同一 Chat 的额外快捷入口。没有工具级 Pin。

Timeline 始终按 `timelineAt` 倒序，ID 仅作为相同时间的稳定兜底。

## 7. 状态表达

### Chat

- 状态图标：错误、工作中、完成待查看或空闲。
- 标题前时间色条：今天红、昨天黄、前天绿、更早蓝。
- 未读统一使用红色语义。
- 工作动画用 1→2→3 点循环；Windows 关闭动画时仍通过内容切换可见。

### Project

Project 行最前显示与 Chat 相同的时间色条；状态槽可同时显示未读红点和工作状态，同时存在时缩小并上下排列。数字区分别显示未读、工作中和总数。

### 诊断

错误类型：额度耗尽、Prompt 拒绝、网络/连接失败、网络重试、疑似无响应、其他错误。错误通知保持显示并提供复制与关闭按钮。

疑似无响应只在用户发送后 120 秒无模型活动、且当前不在等待工具时产生；它是 warning，不是确认失败。

## 8. 时间与额度

执行时间不补零并在固定列居中：`5s`、`1m8s`、`1h2m`、`1d2h`，超过 99 天显示 `>99d`。中断状态不显示；已完成 Chat 使用最后一轮记录的执行时间。

额度显示剩余百分比：

- 5h：`5h  ◯ 65% 18:20`，重置只显示本地时分；
- Week：`Week  ◯ 83% 6d12h`，不足一天显示小时，不足一小时显示分钟；
- Pro 没有 5h 数据时不占位；
- 快照过期后灰环和 `--%`，不推算为 100%；
- 悬停显示精确本地重置年月日时间。

## 9. 搜索、选择和菜单

- 搜索按钮打开覆盖式搜索框，不改变布局；实时过滤；`X` 关闭。
- Project 搜索名称、Folder、ID；Project Chats 只搜当前 Project；Chats 搜项目过滤后的全局内容。
- Project 过滤器为带搜索框的多选菜单；新建 Chat 的 Project 选择器为带搜索框的单选菜单。
- Shift/Ctrl 用于多选；Chats 只移动/复制到 Group，Projects 只移动到 Folder。
- 具体目标使用 Picker 弹窗；菜单限制在视口内，不出现内部滚动条。
- 右键菜单打开时对话预览立即隐藏。

## 10. 对话信息与预览

标题后的预览图标始终显示。悬停后：

- 用户输入使用独立高亮；
- 显示用户、助手与可见中间过程，不显示私有 reasoning；
- 默认将最后一次用户输入定位到预览顶部；
- 鼠标进入预览后可滚动，向上触顶加载更早内容；
- 预览铺满标题栏以下的工具内容区，可 Pin，并可手动关闭；
- 最新页每 3 秒刷新并保留滚动位置。

## 11. Codex 操作

- 打开 Chat：Deep Link。
- 新建无项目或指定 Project Chat：Deep Link；指定 Project 必须用该项目的绝对路径 `path`。Projects 的 Chats 栏按当前 Project 新建，选中无项目对话时新建无项目对话；Chats Tab 弹出项目选择器，默认为无项目对话。
- 改名、Pin/Unpin、归档/恢复：用户启用后走 Codex App MCP。
- 归档 Chat 的改名和 Pin 禁用；恢复后可操作。
- MCP 操作串行化，并在返回后重新扫描 Source 验证。

完整协议见 `CODEX_INTEGRATION.md`。

## 12. 持久化

| Key | 内容 |
| --- | --- |
| `codex-chat-pane.preferences-v1` | 当前 Project、排序/筛选、面板尺寸和打开状态；桌面端主题字段仅作旧版本兼容 |
| `codex-chat-pane.folders-v1` | Folder 的旧版本迁移兼容副本；桌面端配置加载后不再写入 |
| `codex-chat-pane.dynamic-v1` | 浏览器端 Group、成员、各视图顺序与折叠；桌面端迁移兼容副本 |
| `codex-chat-pane.language` | 浏览器端 `zh` / `en`；桌面端由 `settings.toml` 保存 |
| `codex-chat-pane.codex-mcp-consent` | 旧版本内部 MCP 用户选择；桌面端迁移到 `settings.toml` 后删除 |

本地生成的 `config/settings.toml` 保存语言、主题族、亮暗模式、窗口层级、字号、窗口宽度、关闭行为和内部 MCP 同意状态。`config/folders.json` 保存稳定 Folder 数据和桌面端 Group，不保存 Source 时间和运行时区域。这两份本机状态不提交到仓库，首次运行时自动创建。启动时先读原生配置，完成前不得以空前端状态覆盖。导入/导出为可读 TOML，用户选择保存位置；只记录工具本地外观/布局，不写 Codex Pin、归档或未分组项；导入上限 5 MiB。

## 13. 验收不变量

1. 0 Chat Project 合法；后续匹配到同一 Project 时不保留重复实体。
2. 显式 projectless/assignment/project ID 优先于 `cwd`。
3. 工具点击不改变 Codex 最后消息时间。
4. 普通列表只由状态区和接收时间排序；工具点击只通过 `openedAt` 把刚读 Chat 提到已读区顶部。
5. PIN 完全复用 Codex Pin。
6. Archived 与本地 Folder 回收是不同状态。
7. Source 读取失败时保留最后有效快照并显示同步错误。
8. 额度过期不伪造剩余值。
9. 普通扫描不读取对话正文；预览才按需读取。
10. Codex 写操作都有用户授权、动态能力检查和后置验证。
