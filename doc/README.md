# 文档总览 / Documentation index

更新日期：2026-10-29，版本 v0.5.1（[发布说明](releases/v0.5.1.md)）。本文是 CodexChatPane 的全功能入口。功能说明采用本批交付后的行为口径；历史测试、当前检查和实际部署证据单独记录，不以文档更新代表测试通过或 EXE 已更新。

This index covers the complete feature set. Feature descriptions use the delivery behavior; dated test results and actual deployment evidence remain separate. English guides summarize the same behavior; detailed numerical and parser references are in Chinese.

## 阅读入口 / Start here

| 目的 / Purpose | 中文 | English |
| --- | --- | --- |
| 安装与启动 / Start and build | [项目首页](../README.zh-CN.md) | [Project README](../README.md) |
| 操作、菜单、排序、窗口 / User guide | [使用说明](usage.zh-CN.md) | [Usage](usage.md) |
| 数据所有权与功能边界 / Product model | [产品模型](product.zh-CN.md) | [Product](product.md) |
| 启动、监听、并发、缓存 / Architecture | [架构](architecture.zh-CN.md) | [Architecture](architecture.md) |
| 本地数据、MCP、内部测试入口 / Integration | [接入](integration.zh-CN.md) | [Integration](integration.md) |
| 统计公式、日志与请求归属 / Metrics | [对话性能与详情](conversation-metrics.zh-CN.md) | Same technical reference |
| 重置周期、消费归属、计价 / Usage accounting | [额度与消耗](usage-history.zh-CN.md) | Same technical reference |
| 测试、增量编译、安装和发布 / Development | [开发与验证](development.zh-CN.md) | [Development](development.md) |

## 全功能清单 / Feature coverage

| 功能 | 用户行为与数据边界 | 说明入口 |
| --- | --- | --- |
| 源码主界面 | 隐藏 Projects/Chats 切换，仅显示 Projects；保留独立 Chats 页代码；标题名加粗并显示包版本 | [主界面](usage.zh-CN.md#2-主界面) |
| Project / Chat 目录 | 状态库元数据先显示；真实标题、项目关系、归档及 Pin；无项目对话单独归类 | [产品](product.zh-CN.md#4-project-与-chat-归类)、[启动数据流](architecture.zh-CN.md#3-启动与刷新) |
| 动态列表 | 最近天数、搜索、手动加入/移出与规则对话框；P 项目分组可与 N 状态/名称排序组合，方向互斥并记忆 | [主界面](usage.zh-CN.md#2-主界面) |
| 本地组织 | 多层 Folder、交叉 Group、Star、顺序、折叠、多选、拖动及本地回收；不改真实项目目录 | [常用操作](usage.zh-CN.md#3-常用操作)、[菜单](usage.zh-CN.md#6-右键菜单速查) |
| 分栏收起 | Projects 按钮同时收起或展开 Projects / Chats；Chats 无独立关闭按钮，恢复保留内容状态 | [主界面](usage.zh-CN.md#2-主界面) |
| 状态与诊断 | 待回答 `?` 优先于完成待查看、工作中、已读；错误/重连诊断、执行计时、活动日期；回答后恢复实际状态 | [产品状态](product.zh-CN.md#5-排序与状态)、[使用](usage.zh-CN.md#2-主界面) |
| 打开与新建 | Deep Link 定位对话，按项目路径新建；打开文件夹、所属项目、复制标题/链接/诊断 | [Deep Link](integration.zh-CN.md#2-deep-link)、[菜单](usage.zh-CN.md#6-右键菜单速查) |
| Codex 写操作 | 重命名、Pin、Project Pin、归档/恢复；新旧 MCP 管道发现、能力校验、结果回读 | [App MCP](integration.zh-CN.md#3-codex-app-mcp) |
| 归档与菜单 | 归档行只显示恢复；面板不透明、不穿透；所有菜单在外部点击或失焦时关闭 | [菜单](usage.zh-CN.md#6-右键菜单速查) |
| 对话预览 | 默认关闭，开启后按页读取目标正文；兼容压缩与续接，可显示继承正文 | [使用](usage.zh-CN.md#预览对话)、[读取器](conversation-metrics.zh-CN.md#21-自动压缩与历史片段2026-10-15) |
| 选中对话指标 | 当前轮次最近有效结算请求的首字/速度/缓存率，含中断轮次，结算后自动更新；不读全部对话 | [统计](conversation-metrics.zh-CN.md#1-目标与已确定的边界) |
| Σ 总速度 | 运行中主对话近期加权预测求和；工具等待保留，当前压缩排除，无静默过期；`Σ≈125t/s` / `Σ0t/s` / `Σ—` | [Σ 口径](conversation-metrics.zh-CN.md#45-σ-的近期请求预测2026-10-28) |
| 对话详情 | 独立窗口、默认最新轮次、每页 50 轮、全会话汇总、活动记录数、工具汇总、消费、历史 Fast 标识；无请求/活动正文 | [详情](conversation-metrics.zh-CN.md#6-详情页可展示内容) |
| 子代理 | 详情按点击分别读取；消费归入主对话家族并保留主/子构成；主轮次与主速度不混子用量 | [子代理](conversation-metrics.zh-CN.md#34-主对话与子智能体)、[消费](usage-history.zh-CN.md#消耗与计价) |
| 状态栏额度 | 5h/W 剩余百分比，5h 带重置时刻；无 5h 隐藏 W；右侧卡数量带 7/3 天到期标识，整块共用 tips | [额度入口](usage-history.zh-CN.md#入口与显示) |
| 重置记录 | W 当前周期置顶、最近 5 次、每次更多 10 条；自然/提前/卡重置、最后剩余、总 Token、展开模型详情 | [额度周期](usage-history.zh-CN.md#额度周期) |
| 周期识别与稳定边界 | 连续观察确认固定/浮动模式，证据不足保留候选；统一显示/查询边界，历史细化及误周期修正保留关闭状态 | [窗口模式](usage-history.zh-CN.md#窗口模式与候选)、[事件身份](usage-history.zh-CN.md#采用边界与事件身份) |
| 提前重置通知 | 手动关闭、持久去重；重置时间、上一周期、剩余额度、模型 Token/占比/输入输出/缓存/推理/API 金额 | [通知布局](usage-history.zh-CN.md#入口与显示) |
| 消耗范围 | W 单周期/多周期并集/所有周期，或自然日/周/月/全部；两类不取交集，项目/来源/Fast 组合筛选 | [消耗界面](usage-history.zh-CN.md#入口与显示) |
| 模型与趋势 | 顶部总计及模型表，默认 Token 的每日模型堆叠柱，可切 API 金额；tips 完整拆分，颜色稳定 | [模型展示](usage-history.zh-CN.md#入口与显示) |
| 消耗汇总 | 项目/日期展开模型；按对话用真实标题，展开用户提问轮次和子代理；支持列排序 | [轮次与汇总](usage-history.zh-CN.md#入口与显示) |
| API 等价计价 | 未缓存输入/缓存/输出分项，按模型、档位与上下文价格；未知保留，不冒充订阅 credits 或账单 | [计价](usage-history.zh-CN.md#消耗与计价) |
| 懒加载与持久缓存 | 先缓存后补读，按范围/可见行/展开项读取，变化 ID 增量更新，跨机器/账号/数据源隔离 | [架构缓存](architecture.zh-CN.md#缓存与失效)、[消费缓存](usage-history.zh-CN.md#存储与性能) |
| JSONL / ZST / 续接 | 共用逻辑读取器；完整行游标、历史链、线程身份与响应去重；继承正文可预览但不重复计量 | [解析](conversation-metrics.zh-CN.md#7-解析流程与降级) |
| 窗口与外观 | 多主题、明暗、中文/英文、分级字号、列宽/分栏；普通/随 Codex/置顶、托盘、双屏物理位置记忆 | [设置](usage.zh-CN.md#4-设置) |
| 导入导出与诊断 | 只导出工具配置和整理关系；日志等级与日志路径；可选本机调试端口，默认关闭 | [设置与排障](usage.zh-CN.md#4-设置) |
| 内部数据测试 | 第二实例提交 JSON 测试事件；已接通知预览，内存展示，不修改真实额度或关闭记录 | [测试协议及例子](integration.zh-CN.md#6-内部数据测试入口) |
| 构建与发布 | 前端指纹/Cargo 增量缓存；本地固定 EXE，公开附件带版本；部署核对进程与哈希 | [开发交付](development.zh-CN.md#3-构建便携版-windows-exe) |

## 统计粒度 / Accounting units

| 粒度 | 含义与用途 |
| --- | --- |
| 模型请求 | 一次模型响应，用 response ID 去重；工具返回后的再次生成是新请求，未必是新轮次 |
| 技术轮次 | 日志 turn ID；用于对话详情、最新请求指标与执行状态 |
| 用户提问轮次 | 消耗详情从首次提问到最终回复；有明确依据的中断继续/自动续跑可合并多个技术 turn，未识别请求单列 |
| 主对话自身 | 仅目标 thread 的请求，排除继承与其他智能体；用于主速度和主轮次 |
| 对话家族 | 主对话与所属子代理的各自请求总和；用于消费项目/对话归属，不重复累计子树 |
| W 周期 | 以额度观察中的重置为边界，包含耗尽及未耗尽即重置；不等于订阅计费周期 |
| 自然时间 | 本地日/周/月边界，按请求结算时间归属；与 W 周期是互斥的选择方式 |

总 Token = 输入 + 输出；缓存输入包含在输入内，推理包含在输出内。速度只统计输出；缓存率为缓存输入总和 / 输入总和。API 等价金额是所用价表的估算，不是逐对话 credits、历史订阅账单或用量百分比的固定换算。未知值用 `—`，尚未加载用 `…`，有证据的零才显示 `0`。

## 接手顺序与约束 / Handoff

1. 先读本页和对应使用说明，确认是目录、状态、最新请求、完整详情、消费范围还是在线额度的问题。
2. 按[功能到代码映射](product.zh-CN.md#8-功能文档与代码对应)定位共享入口，再查看目标函数和调用者；不要为新显示另建全库扫描。
3. 统计问题对账同一范围的请求 ID 和原始分子/分母，轮次映射失败不丢用量。压缩开始信号缺失、账号归属不明、未知价格及日志缺口要保留限制。
4. 更新对应专题及中英文用户文档；文档中的规则、测试用例和实际运行结果分开，不把历史测试数字当当前验收。
5. 按[开发说明](development.zh-CN.md)选择必要验证。只有源码/资源变化需要构建；本地部署、Git 提交、同步 release 与 GitHub 发布分别记录。

速度预测不使用 OpenTelemetry、内部 IPC、流量代理或额外 app-server。已有额度查询的 MCP / CLI 回退保持独立。完整功能说明不等于所有原生交互、全部账号和全历史性能均已验证，具体限制见专题和开发记录。
