# 产品模型

[English](product.md) | [简体中文](product.zh-CN.md)

本文定义 CodexChatPane 当前版本的用户可见行为和数据边界。代码实现细节见 [architecture.zh-CN.md](architecture.zh-CN.md)，Codex 接入细节见 [integration.zh-CN.md](integration.zh-CN.md)。

## 1. 产品目标

CodexChatPane 解决的是本机 Codex Desktop 中 Project 和 Chat 较难同时浏览、筛选和整理的问题。它提供一个本地辅助视图，不替代 Codex Desktop，也不建立新的远端数据源。

## 2. 核心实体

| 实体 | 来源 | 含义 |
| --- | --- | --- |
| Project | Codex | Codex 中的项目身份、名称和路径 |
| Chat | Codex | 对话身份、标题、归档状态、所属 Project 和时间 |
| Folder | CodexChatPane | 本地的层级整理方式 |
| Group | CodexChatPane | 可交叉复用的本地集合 |
| Timeline | Codex 事件与 rollout | 最近活动、执行时间、当前状态和额度 |
| Archive | Codex | Codex 原生归档状态，不等同于本地 Folder 回收站 |

## 3. 数据所有权

- Codex 拥有 Project、Chat、归档、Codex Pin 和未读状态。
- CodexChatPane 拥有本地 Folder、Group、星标、手动顺序、主题和窗口设置。
- 工作状态、执行时间、额度和诊断信息由 Codex 数据与事件推导，工具不改写原始事件。
- 对话正文只在预览时按需读取，不作为本地快照保存。

## 4. Project 与 Chat 归类

Chat 的 Project 归类遵循以下优先级：

1. 明确的无 Project 标记。
2. Codex 全局的 Thread-Project assignment。
3. Thread 自身的 `project_id`。
4. Thread 当前工作目录与 Project 路径匹配。
5. 仍无法归类时进入合成的“未分类对话” Project。

合成 Project 只是界面占位，不写回 Codex。

## 5. 排序与状态

- 默认按活动区域和接收时间排序，不把本地点击时间当作 Codex 活动时间。
- 可切换名称排序、Project 过滤、日期范围和归档视图。
- Chat 状态可以包含未读、工作中、已完成、失败、被中断和无响应诊断。
- Project 和 Chat 的 Pin 优先级来自 Codex；本地 Group 不改变 Codex Pin。

## 6. 本地持久化

应用会生成两类本地文件：

- 用户配置目录下的 `settings.toml`：语言、主题、字号、窗口模式、窗口尺寸、日志级别和 MCP 同意状态。
- 用户配置目录下的 `folders.json`：Folder、Group、Project/Chat 的本地归属、星标、顺序和折叠状态。

筛选、分栏高度和部分高频界面状态保存在浏览器 `localStorage`。这些状态不会同步到 Codex。

## 7. 明确不做的事

- 不上传对话、rollout、日志或本机数据库。
- 不把本地 Folder 自动转换成 Codex Project。
- 不把合成 Project 写回 Codex。
- 不承诺兼容 Codex Desktop 的公开稳定 API 之外的所有未来版本。
- 当前不承诺 macOS 或 Linux 的窗口联动行为。
