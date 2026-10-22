# 对话性能统计与详情页：实现交接

状态：功能已实现，支持压缩日志；2026-10-22 增加中断与当前轮次的已结算请求统计、自动更新，以及状态栏最新有效请求的增量读取。原生窗口交互未人工验收。原始算法调查日期：2026-10-05。

本文记录用户已确定的统计口径、本地日志证据、解析规则、页面范围和验收条件。接手者应先读第 1、4、5、10 节；遇到数据兼容问题再查其余章节。

## 目录

1. [目标与已确定的边界](#1-目标与已确定的边界)
2. [数据来源与版本差异](#2-数据来源与版本差异)
3. [日志字段与关联键](#3-日志字段与关联键)
4. [生成速度算法](#4-生成速度算法)
5. [其他指标与汇总口径](#5-其他指标与汇总口径)
6. [详情页可展示内容](#6-详情页可展示内容)
7. [解析流程与降级](#7-解析流程与降级)
8. [实测基准](#8-实测基准)
9. [现有代码入口与最小接入](#9-现有代码入口与最小接入)
10. [验收与接手顺序](#10-验收与接手顺序)
11. [参考资料](#11-参考资料)

## 1. 目标与已确定的边界

用户希望了解模型生成速度，而非完成整个代理任务的效率。根本障碍是：日志有输出 token 用量，但不是每次请求都有完整的生成起始时间。

已确定：

- 状态栏采用 `1.8s·62t/s·93%`，依次为首 token 延迟、估算生成速度、缓存命中率；tips 解释口径。
- 选中对话后，在新轮次、模型请求结算及轮次终止时更新状态栏统计与 tips；工具输出和普通时钟 tick 不触发全量重算，详情窗口按需读取。
- 输出 token 包含推理、正文、模型生成的工具调用参数；工具返回内容不属于模型输出。
- 排除工具执行、审批、等待子智能体等阻塞时间。
- 主对话和子智能体分别统计，子智能体用量不混入主对话速度。
- 用户接受按可用日志边界估算速度。缺少有效起点的请求，从速度的分子和分母同时排除；保留在用量总数中。
- 速度同时提供 token 覆盖率和有效请求数。没有可用样本显示 `—`。

已确定实现方式：状态栏使用当前轮次最近一次有效、已结算的模型请求，包括进行中和中断轮次，不汇总该轮全部请求；详情默认显示该轮汇总。首 token 必须属于所选请求：首个请求可使用整轮原生计时，缺失时可按精确轮次开始到首条正文输出起点估算；正文前已有工具调用时不补算。后续请求没有可靠独立首字边界则显示 `—`，不能套用整轮 TTFT。tips 明确标记估算。指标靠右，紧邻额度左侧，时间保留左侧且不压缩。同一轮次/状态/请求结算标识只触发一次自动读取，隐藏窗口恢复后补读。

普通对话右键最后一项与状态栏提供详情入口，桌面端使用独立、可缩放的无系统标题栏窗口，内部提供拖动与窗口按钮。轮次表与轮次选项按开始时间倒序展示；详情仍可手动切换其他轮次或全会话已完成轮次汇总；工具汇总明确标注会话范围，子智能体各自读取详情。归档右键仍只显示恢复对话。上下文容量、压缩统计暂不新增。

2026-10-08 用户已授权调用子代理实现速度和详情两个功能；统计数据仅保存在内存，不写入 Codex 数据库或项目配置。

## 2. 数据来源与版本差异

复用应用已有的 Codex home 定位逻辑；默认 `%USERPROFILE%\.codex`，可能由 `CODEX_HOME` 修改。所有 Codex 原始数据只读访问。

| 来源 | 用途 | 已知限制 |
| --- | --- | --- |
| `state_5.sqlite` 的 `threads` | 按 ID 定位 `rollout_path`，读取标题、模型、项目目录、归档信息 | 文件名和字段随版本变化；数据库中的模型主要反映当前状态 |
| `sessions/**/rollout-*.jsonl` / `*.jsonl.zst` | 轮次、逐请求用量、输出计时、工具、子智能体关系 | 日志可能在运行期间被压缩；正在写入时末行可能不完整；同一对话可能有续接文件 |
| `archived_sessions/**` | 状态库路径失效时查找归档记录 | 优先按线程 ID 定向定位，不全盘扫描正文 |
| `thread_history_1.sqlite` | 补充轮次和 item 元数据、已有预览能力 | 本次观察到投影落后于 rollout；不能仅靠它判断最新状态 |
| `logs_2.sqlite` / Desktop 日志 | 有数据时用于诊断 | 本次该日志库查不到记录；不作为指标必需来源 |

历史库实际观察到的字段：

- `thread_turns`：`thread_id`、`turn_id`、`status`、`started_at`、`completed_at`、`duration_ms`、rollout 序号和字节偏移等。
- `thread_items`：`thread_id`、`turn_id`、`item_id`、`item_type`、`item_json`、`started_at_ms`、`completed_at_ms` 等。

本次实测样本由 CLI `0.160.0` 记录，具有顶层 `token_usage_record`。此前检查的另一个较旧会话只有 `event_msg/token_count`，不可假设所有历史会话都有逐请求用量。应检测字段能力，而非只判断版本号。

读取 SQLite 使用只读连接；正在运行的数据库使用正常 SQLite/WAL 读取方式，避免单独复制主库而漏掉 WAL。

此前通过外部 Python 进程连接 Desktop 内部 IPC，已收到会话快照、增量文本和 token 更新，但仍未获得完整的隐藏推理计时。本功能的最小方案不依赖 IPC、MCP 或常驻监听，也无需由 Codex 自身调用 MCP。

### 2.1 自动压缩与历史片段（2026-10-15）

- `rollout_log.rs` 统一解析逻辑路径：每次打开优先明文，再尝试 `.jsonl.zst`；遇到文件不存在时有限重试。保留已打开的文件句柄，允许本次读取在重命名后完成。不改写或解压覆盖 Codex 文件。
- Zstandard 使用内置 Rust 依赖流式解压，不依赖机器上的 `zstd.exe`，也不把整份日志常驻内存。长度、预览游标和 `history_base.end_byte_offset` 全部采用解压后的字节。压缩流定位需要从头解码，首次读取大文件可能较慢。
- 压缩长度缓存仅保存物理路径、大小、修改时间和逻辑长度；预览/活动缓存也包含实际来源签名。明文与压缩格式切换、替换或追加后重新读取。监听同时识别两种后缀。
- 历史父片段可属于另一个对话；预览允许显示继承历史，指标解析仍按目标 `thread_id` 隔离。继承历史或子代理的旧累计快照不作为主对话用量；当前对话实际请求中重新发送的上下文输入正常计入。
- 活动投影按当前文件名中的续接片段 ID 查询，再映射回对话 ID；已完成压缩文件优先使用对应片段的 SQLite 投影，避免启动时解码全部归档。投影缺失或未完成时读取日志。
- 外部全量调查：771 个索引对话均有明文或压缩日志，903 个日志片段可解压，父历史引用全部可定位。761 个对话至少有一种有效用量记录；另外 10 个未记录有效用量且索引 `tokens_used=0`。旧日志缺少逐请求计时的情况仍显示缺失值，不能用整轮耗时替代生成时间。
- 修复验证：52 项默认 Rust 测试通过；独立测试进程逐个读取 771 个对话统计，零读取失败；固定主/子代理请求数、有效 token 和时间分母与第 8 节一致；5 个真实对话的预览及翻页通过，包含全部 4 个跨对话父历史引用。活动快照覆盖 564 个主对话，debug 首次约 7.9 秒、第二次约 0.6 秒。未执行归档等真实写操作或原生界面验收。

## 3. 日志字段与关联键

JSONL 一行一个对象；常见外层字段为 `timestamp`、`ordinal`、`type`、`payload`。按文件记录顺序解析；`ordinal` 不是跨文件全局唯一键。时间计算统一为毫秒，展示时转换为用户时区。

### 3.1 会话、轮次与模型请求

三种粒度必须分开：

- 会话：`thread_id`。
- 一轮用户交互：`turn_id`，由 `task_started` / `task_complete` 及带 ID 的事件关联。
- 一次模型请求：逐请求用量中的 `response_id`。工具执行后再次调用模型，会出现新的请求，但通常仍在同一轮。

| 记录 | 关键字段 | 用法 |
| --- | --- | --- |
| `session_meta` | `id`、`session_id`、`cli_version`、`cwd`、`thread_source`、父子关系字段 | 确认日志身份与来源 |
| `turn_context` | `turn_id`、`model`、`effort`、`cwd` | 轮次模型和推理强度；设置改变时按顺序更新上下文 |
| `event_msg/task_started` | `turn_id`、`started_at`、`model_context_window` | 建立轮次；不等于模型开始解码 |
| `event_msg/task_complete` | `turn_id`、`duration_ms`、`time_to_first_token_ms`、`started_at`、`completed_at` | 原生轮次耗时和 TTFT |
| `event_msg/item_completed` | `thread_id`、`turn_id`、`item`、`started_at_ms`、`completed_at_ms` | 输出和工具 item 的生命周期 |
| `token_usage_record` | 见下一节 | 逐请求用量与累计值 |

`started_at` / `completed_at` 在实测中为秒；`*_ms` 和 `duration_ms` 为毫秒。优先明确单位字段；旧字段兼容可复用现有 `seconds_or_millis`，不要把所有数字都视为毫秒。

### 3.2 逐请求 token 用量

`token_usage_record.payload` 实际包含：

```text
thread_id, turn_id, session_id, root_turn_id, response_id
usage
turn_token_usage
thread_token_usage
```

三种 usage 对象具有以下计数字段：

```text
input_tokens
cached_input_tokens
cache_write_input_tokens
output_tokens
reasoning_output_tokens
total_tokens
```

规则：

- `usage` 是本次请求用量；按 `(thread_id, response_id)` 去重后汇总。
- `turn_token_usage` / `thread_token_usage` 是累计值，用于核对，不再逐条相加。
- 同一请求重复记录内容相同则忽略；发生冲突时标记数据不完整，不静默相加。
- `reasoning_output_tokens` 已包含在 `output_tokens` 内；缓存输入已包含在 `input_tokens` 内。
- 同时出现 `token_count` 与 `token_usage_record` 时，以后者为逐请求来源，前者不重复计量。
- 如果读取的是续接日志的一部分，本文件逐请求和可能小于线程累计值。不能拿跨文件累计 token 去除以当前文件的生成时间。

### 3.3 输出和工具记录

输出计时来自 `item_completed.item.type` 为 `Reasoning` / `AgentMessage` 的记录。正文阶段可见 `phase: commentary` 或 `final_answer`。正文、推理摘要、加密内容本身无需用于 token 计数。

工具调用与返回出现在 `response_item.payload`：

| 调用类型 | 返回类型 | 配对字段 |
| --- | --- | --- |
| `function_call` | `function_call_output` | `call_id` |
| `custom_tool_call` | `custom_tool_call_output` | `call_id` |

完整调用记录的外层 `timestamp` 可作为参数生成完成的近似终点，不能当作参数开始生成时间。调用与返回的时间区间用于识别工具阻塞；生成参数属于模型阶段。

工具 item 还可见 `McpToolCall`、`CommandExecution`、`FileChange`、`CollabAgentToolCall`、`SubAgentActivity`。MCP 有 server/tool/status，命令有 command/exit_code/status，文件修改有 changes/status。只展示实际存在的字段；参数或正文可能缺失、被加密或截断。

### 3.4 主对话与子智能体

父日志的 `SubAgentActivity` 具有 `agent_thread_id`、`agent_path`、`kind`。子日志元数据实际具有：

```text
thread_source = "subagent"
parent_thread_id
forked_from_id
source.subagent.thread_spawn.parent_thread_id
source.subagent.thread_spawn.agent_path
source.subagent.thread_spawn.agent_nickname
```

子智能体的 `session_id` 可能仍是主对话 ID，`root_turn_id` 也指向主对话原始轮次。因此：

- 指标按当前智能体的 `thread_id` 过滤；`session_id` / `root_turn_id` 仅用于建立关联。
- 子日志可能包含继承的父对话历史、多个 `session_meta` 和父轮次记录。只有属于该子线程的请求用量、轮次和事件进入其统计。
- 父子关联从结构化字段读取，不靠标题、提示词或正文猜测。
- 子智能体结果被主模型读取后，属于主请求的输入；这不是重复计算子智能体输出。

## 4. 生成速度算法

### 4.1 最终采用的估算口径

对每个具备可用计时的模型请求 `i`：

```text
N_i = usage.output_tokens
S_i = 此请求最早的有效 Reasoning / AgentMessage started_at_ms
E_i = 此请求最后的模型输出完成时间
D_i = (E_i - S_i) / 1000

估算速度 = Σ(N_i - 1) / ΣD_i
```

`E_i` 取同请求输出 item 的 `completed_at_ms` 与工具调用参数完成记录的时间戳的最大值；不取工具返回、工具执行完成或整轮完成时间。这里每次请求减 1，沿用首 token 到末 token 区间的口径；日志起点只是近似观测边界，不能因此声称测到了服务端首个隐藏 token。

第一版纳入条件，与实测脚本一致的核心条件：

1. 请求身份、所属线程与轮次、输出用量明确且未重复。
2. 至少有一个输出 item 的 `completed_at_ms > started_at_ms`，提供有效起点；相等或缺字段不能单独提供起点。
3. 若该请求有推理 token，需存在有效的推理计时，不能只拿正文起点代替。
4. 请求终点晚于起点，`N_i > 1`。
5. 若该请求已有输出调用早于候选起点，说明输出前段缺失，排除该请求。

实现还须校验时间关联：请求区间不能越过前一请求边界；已识别的工具等待不能落入区间；不同请求生成区间不应因错误关联而重叠。发现这些异常时排除对应请求并记录原因，不靠删除空档或裁剪时间制造高速度。

请求结算后即可参加对应轮次的速度计算，不要求整轮为 completed。正在生成但没有对应 usage 记录的尾段不参加分子、分母；中断不会抹掉之前已结算的有效样本。

生成期间的未知停顿保留在该区间内，不用阈值把长间隔判断成排队并删除。日志边界无法剥离全部网络缓冲、隐藏推理前段或服务端停顿。

### 4.2 如何把输出关联到请求

实测样本中，`token_usage_record` 按响应完成顺序出现；输出 item 与调用记录位于上一条用量记录之后、本条用量记录之前，可建立候选请求段。

最小解析过程：

```text
对目标 thread 的当前 turn：
    收集模型输出 item 和工具调用完成记录
    遇到 token_usage_record：
        以 response_id 结束候选请求段
        校验 thread/turn、用量去重、时间边界与工具等待
        保存有效样本或排除原因
        清空本请求候选输出
```

这是经过该样本验证的顺序关联法，不是保证适用于所有 Codex 版本的协议。晚到的工具完成事件不属于新的模型输出；带有明确 thread/turn 元数据的事件优先使用其身份字段。若出现交错响应、缺失用量记录、无法归属的输出或坏行破坏关联，相关段降级为无速度，不跨段拼接。

### 4.3 覆盖率与解释

```text
token 覆盖率 = 有效请求的 ΣN_i / 当前范围所有去重请求的 ΣN_i
请求覆盖 = 有效请求数 / 当前范围有用量的请求数
```

覆盖率使用原始 `N_i`，不是减 1 后的分子。若所有请求的用量总数本身不完整，覆盖率也只能相对于已读取记录，必须另外显示“记录不完整”。

这是有计时样本的生成速度，不是全请求精确平均值。工具调用专用请求更容易缺起点，缺失并非随机，不能以覆盖率高就承诺误差小，也不设置未经验证的“可信度百分比”。

### 4.4 已弃用的方法

以下结果只留作防止接手者回退错误算法，不用于界面：

- 全部输出 token / 仅正文或推理 item 时长：遗漏参数生成，却保留其 token，偏高风险大。
- 全轮耗时减去工具 item 完整运行时长：后台命令与模型可能并行，存在过度扣时。
- 全轮耗时减去调用阻塞：仍包含排队、输入处理和调度，不符合用户要的生成速度。
- 只对可见正文分词：不包含隐藏推理，且已有真实 usage，不采用。
- 用上一工具返回到下一输出完成作为纯生成时间：包含首 token 前开销，不采用。
- 用固定首 token 延迟或其他请求的平均延迟补齐缺失起点：没有证据支持，不采用。

## 5. 其他指标与汇总口径

| 指标 | 计算 / 来源 | 展示限制 |
| --- | --- | --- |
| 原生首 token 延迟 | `task_complete.time_to_first_token_ms / 1000` | 一轮一个；不等于第一段可见正文出现时间 |
| 估算首 token 延迟 | 首个有效 AgentMessage 的 `started_at_ms` 减去 task_started 记录的精确时间戳 | 无原生值时使用；首条正文前已有工具调用或缺少边界则不估算，`ttftEstimated=true`，不能声称等同原生首 token |
| 会话 TTFT 汇总 | 已完成轮次的可用值，显示均值/中位数和样本数 | 含估算样本时标记估算；无值不补 0；勿把轮次 TTFT 当成每次请求 TTFT |
| 缓存命中率 | `Σcached_input_tokens / Σinput_tokens` | 请求去重后计算，不平均每次百分比；分母 0 时 `—` |
| 未缓存输入 | `input_tokens - cached_input_tokens` | 负数代表异常，不能静默截为正常结果 |
| 输出 / 推理用量 | `Σoutput_tokens`；其中推理为 `Σreasoning_output_tokens` | 推理为子集，不再相加 |
| 轮次总耗时 | 优先原生 `duration_ms` | 包含工具与其他等待；不能标成生成耗时 |
| 工具调用阻塞 | 同 `call_id` 调用完成记录到返回，区间合并去重 | 后台命令返回后不继续扣阻塞时间；yield/恢复需按实际返回边界处理 |
| 工具运行耗时 | 对应工具 item 的计时 / duration | 与阻塞耗时分开展示；后台、并行工具不可直接相加成总等待 |
| 主智能体等待子智能体 | 实际 `wait_agent` 等阻塞调用区间 | 不使用子智能体从创建到完成的整个时长 |
| 生成速度汇总 | 所选范围所有有效样本的分子和 / 分母和 | 不平均每次 t/s；跨模型时提供模型分组 |
| 团队用量（可选） | 按各线程独立请求去重后相加 | 主对话指标默认独立；不将并行线程速度相加 |

补充：

- 累计输入 token 包含重复发送的历史上下文，不等于当前上下文窗口占用。
- 模型上下文容量可从原生字段展示；用请求输入量估计窗口占用时需另行验证语义。
- 工具状态 completed 不代表业务成功；结合明确的 failed、exit_code 或结构化错误展示，不从任意文本猜测。
- 轮次数按独立 turn ID 计数，不按 UserMessage item 数或子智能体消息条数计数。
- 运行中和中断轮次按已结算请求显示独立用量与速度；全会话“已完成轮次”汇总仍不纳入它们，不把不完整轮次耗时与完整轮次混算。

## 6. 详情页可展示内容

建议四部分；这是可实现范围，不要求一次加入全部字段或图表。

### 总览

- 统计范围：最新轮次 / 所选轮次（含进行中、中断）/ 全会话已完成轮次；显示截至时间和数据是否完整。
- 标题、项目目录、创建/更新时间、归档状态、模型与推理强度。
- 首 token、估算速度、缓存命中率。
- 输入、缓存输入、未缓存输入、输出、其中推理 token。
- 轮次数、模型请求数、总轮次耗时、有效样本数和 token 覆盖率。

### 轮次明细

每轮保留 `serviceTier`：读取本线程的 `event_msg.thread_settings_applied.payload.thread_settings.service_tier`，在轮次首次建立时保存当时值。`priority` / `fast` 在模型列显示 `· Fast`；`default`、缺失、null 或其他未知值不显示标识。后续配置不追改已有轮次；切换日志线程身份时清空配置，不能继承父线程设置。同线程续接保留已知配置。该字段表示客户端设置，不是服务端逐请求实际服务档位。

每行显示时间、模型、状态、首 token（原生优先，否则按上述条件估算）、速度、覆盖率、缓存命中率、输出 token、总耗时、工具阻塞。展开后显示请求级用量、排除原因和时间线。

时间线可展示推理、正文、工具、等待和子智能体活动。保留并行轨道与“未分类”空档；不能把所有时间强行分成互斥的“生成 + 工具”两块。

### 工具统计

按工具名聚合调用次数、已记录运行耗时、阻塞耗时、明确失败次数；可查看具体调用和错误。外层 `exec` 与内部命令可能是同一次工作，不把两层耗时重复汇总；关联不完整时分组展示而非相加。

可补充已记录的命令、退出码、文件路径和修改项。日志里的文件修改不等于当前 Git diff；本页不需要扫描当前代码库来“验证”历史修改。

### 子智能体

名称/路径、父线程、模型、状态、轮次、输入/输出、缓存、原生首 token、估算速度、覆盖率、耗时；点击进入同结构详情。只汇总各自身份下的数据，继承的父历史不计量。

### 状态栏与 tips

- 紧凑格式：`1.8s·62t/s·93%`；小数与整数采用一致舍入规则，99.52% 显示为 100% 时 tips 保留小数。
- 单项缺失只替换该项：例如 `2.7s·—·99%`。
- 状态栏 tips 显示“当前轮次最新有效请求”、状态、响应 ID、首 token 来源、估算生成速度和缓存命中率。请求计时无效或未结算时不采用；同轮可保留最近有效请求，没有有效请求时不回退到旧轮次。第 8 节固定轮次仍用于复算，不一定是当前最新轮次。
- 切换对话时保持所选 ID 与异步结果一致，晚返回的旧对话结果不得覆盖当前状态栏。

## 7. 解析流程与降级

状态栏调用 `get_chat_metrics({threadId, latestOnly:true})`，详情显式传 `latestOnly:false`（省略也兼容完整读取）。前端两种范围的在途请求分别合并，完整详情不会覆盖状态栏缓存。

`metrics/latest.rs` 独立维护一个选中线程的游标：首次从明文尾部定位当前轮次起点，压缩文件顺序扫描定位；只有轮次跨续接片段时才追溯到该轮起点。随后仅解析该轮；明文追加只读取新增完整行，新轮次清空旧轮次状态。文件签名未变时复用结果，替换、截断、压缩及历史来源改变时失效；追加前核对游标尾部字节，避免把替换文件接在旧状态上。半行不推进游标。首读压缩文件仍需顺序解压，不能承诺常数时间随机定位。状态栏不读取子代理文件，也不返回历史轮次、工具汇总或活动正文。

以下完整历史解析流程仅用于详情：

1. 通过状态库定位目标线程当前 rollout，确认文件元数据身份；只读打开。失败时按目标 ID 定向查 sessions/archived_sessions。
2. 记录文件长度与修改时间；本次读取限定快照范围，完整行流式解析。不把增长中的尾部当作完整请求。
3. 读取上下文并建立 thread/turn 身份；处理子日志继承历史和续接文件。文件变化或截断后按现有缓存模式失效。
4. 逐请求用量按 `(thread_id, response_id)` 去重；累计值用于交叉核对。
5. 解析输出计时和工具配对，构造候选请求段；按第 4 节纳入或排除。
6. 分别聚合轮次、线程、模型与子智能体；输出原始分子/分母和缺失原因，格式化留给前端。

降级要求：

- 尾部半行留待下次读取；中间坏行记录不完整，并使穿过该坏行的请求段失效。
- 重复日志、继承记录和多个累计快照不能重复计量；若无法定位完整历史，展示当前覆盖范围。
- 只有 `token_count` 的旧日志：可展示明确作用域的用量快照。没有可靠逐请求归属时，速度显示 `—`，不能将重复 last usage 当新请求累加。
- 缺少原生 TTFT 时显示 `—`，不拿首条完整正文的落盘时间冒充。
- 缺少调用返回、响应交错、时间倒退等情况，记录具体排除原因；不插补起点。
- 遇到未识别的新记录类型跳过并保留诊断，不让 token 用量事件变成新的用户活动或未读消息。
- 子线程日志缺失时显示缺失状态，不以 0 用量表示。

请求详情的建议诊断字段：`responseId`、`outputTokens`、`reasoningTokens`、`startMs`、`endMs`、`included`、`excludedReason`。汇总至少保留总输出、有效输出、有效请求数、总请求数、生成区间总毫秒数；无需新增数据库或常驻服务。

## 8. 实测基准

这些数值对应固定历史轮次，不是该会话持续增长后的最新全量统计。证据来自外部 Python 只读查询 SQLite 和 JSONL，不是 Codex 内置 MCP 返回。

主线程：`01a109ee-08e0-7e10-a737-a8e2d4cf3b5d`。

目标轮次：`01a109fc-3e8b-7342-9504-c5b6144be793`。

当前机器的主文件：

```text
C:\Users\jh3ma\.codex\sessions\2026\10\05\rollout-2026-10-05T10-39-17-01a109ee-08e0-7e10-a737-a8e2d4cf3b5d.jsonl
```

接手时通过 state 库重新定位，不把此绝对路径硬编码进应用；不将原始对话日志提交到仓库。

| 对象 | thread ID | 总请求 / 有效请求 | 总输出 / 有效输出 token | 有效区间秒数 | 估算 t/s | token 覆盖率 |
| --- | --- | ---: | ---: | ---: | ---: | ---: |
| 主对话 | `01a109ee-08e0-7e10-a737-a8e2d4cf3b5d` | 82 / 61 | 17341 / 14945 | 718.853 | 20.71 | 86.18% |
| backend | `01a109fc-84c3-72a2-a586-a1d939f558b0` | 65 / 38 | 30364 / 19794 | 635.213 | 31.10 | 65.19% |
| frontend_environment | `01a109fc-a1fb-74c2-9f87-8e92d56266b1` | 52 / 23 | 26028 / 16088 | 571.952 | 28.09 | 61.81% |
| frontend_page | `01a109fc-c1b0-7e00-b76d-054cb9439156` | 32 / 17 | 26321 / 21706 | 654.683 | 33.13 | 82.47% |
| seed_data | `01a109fd-464a-7fb1-909c-e67f9de108e7` | 26 / 13 | 9592 / 7782 | 233.886 | 33.22 | 81.13% |
| integration_review | `01a10a05-6ce3-7c82-b95c-635493bd68d3` | 26 / 13 | 3842 / 2838 | 94.508 | 29.89 | 73.87% |

子线程行对应各自该次任务的首个用量所属轮次，不包括继承的父轮次。实现额外兼容校验若改变样本，应说明实际排除原因，不为匹配此表放宽条件。

主轮次可核对：

```text
input_tokens          = 14995854
cached_input_tokens   = 14923904
output_tokens         = 17341
reasoning_output_tokens = 5294
time_to_first_token_ms  = 2663
duration_ms            = 1398216
缓存命中率             = 99.52%
速度分子               = 14945 - 61 = 14884
速度                   = 14884 / 718.853 ≈ 20.71 t/s
```

82 条逐请求用量之和等于本轮 `turn_token_usage`；父子线程的 response ID 无重叠。主轮次 81 次工具调用均有配对返回，候选生成区间未与这些工具阻塞重叠。

排除的 21 次请求共有 2396 个输出 token：20 次只产生工具调用，没有推理/正文计时；1 次存在推理 item，但起止时间相同。该次仍有 31 个推理 token，证明 0ms 不代表实际没有生成耗时。例如两次 `spawn_agent` 分别输出 282、298 token，只有完整参数出现的时间，没有参数开始生成的时间。

## 9. 现有代码入口与最小接入

以下路径在本次仓库检查中存在，修改前通过 CodeGraph 重新定位当前符号与调用者：

- `src-tauri/src/source.rs`：状态库和 SourceSnapshot，已有 `seconds_or_millis`；复用 home、thread、rollout 路径定位。
- `src-tauri/src/rollout_log.rs`：共用明文/压缩读取、逻辑字节定位及实际文件签名。
- `src-tauri/src/source/rollout.rs`：活动摘要、问题状态及 `metricsRevision`（当前轮次末次 token_usage_record.response_id）；新轮次清空结算标识。文本仅临时解析，不持久化完整性能内容。
- `src-tauri/src/preview.rs`：按需分页读取模式可参考；详情统计不能只对一页正文分词。
- `src-tauri/src/metrics.rs`：流式解析、逐请求计量、工具阻塞区间和独立子线程统计；测试包括合成数据与只读真实样例。
- `src-tauri/src/lib.rs`：`get_chat_metrics` Tauri command 与窗口入口。
- `src/app.js`：`loadSelectedChatMetrics` / `statusChatMarkup` / `openChatMetrics`。点击对话按需读取，状态栏固定选中对话；悬停不发起读取。
- `src/i18n.js`、`src/styles.css`：文本与界面样式。

最小方向：后台按目标线程读取并返回指标摘要/明细，前端只负责范围选择、展示和格式化。复用现有按需读取与缓存失效模式；不要为了历史统计增加实时 IPC 订阅、全库扫描、独立服务或持久化统计库。活动检测与指标聚合的职责保持清晰，尤其不能让 `token_count` 刷新未读/活动状态。

### Rust / 前端接口

实现采用 `get_chat_metrics` Tauri command；前端调用 `nativeInvoke('get_chat_metrics', {threadId})`。后端验证线程 ID，使用 `spawn_blocking` 只读解析，返回 camelCase 字段。`open_chat_metrics_window` 创建/复用 `chat-metrics` 窗口，路由为 `index.html?metrics=<threadId>`，复用时发送 `chat-metrics-open`。该窗口只读取工具主题配置及目标统计，不加载全库快照、额度或实时轮询，不保存主 pane 的窗口尺寸；关闭只关闭详情窗口。

```text
页面：threadId, title, cwd, model, effort, archived, createdAt, updatedAt,
      asOf, complete, warnings, latestTurnId, summary, turns, tools, subagents
summary：inputTokens, cachedInputTokens, outputTokens, reasoningOutputTokens,
         totalTokens, requestCount, validRequestCount, validOutputTokens,
         generationMs, tokenSpeed, cacheHitRate, coverage, ttftMs,
         ttftMedianMs, ttftSampleCount, ttftEstimated, durationMs, toolWaitMs,
         turnCount, completedTurnCount
turn：turnId, model, effort, status, startedAt, completedAt, metrics,
      requests, activities
request：responseId, inputTokens, cachedInputTokens, outputTokens,
         reasoningTokens, startMs, endMs, included, excludedReason
tool：name, count, failures, runtimeMs, waitMs
subagent：threadId, name, path, model, status, summary, error
```

时间字段统一为 Unix 毫秒，`cacheHitRate` / `coverage` 为 0–1；未知指标为 null，0 表示确实为零。顶层 summary 汇总完成轮次，供手动选择全会话时使用；`latestTurnId` 取开始时间最新的轮次，不按 completed 过滤。运行中轮次的速度只用已结算请求；主线程和每个子代理的统计始终独立。

子智能体详情由相同 command 按其线程 ID 再读取；父页仅返回直接子智能体摘要，不递归展开整棵树。活动 detail 只展示有限长度的明文命令、路径和结果摘要。统计不依赖解密消息；加密信件的明文信头不代表其正文可读。

后台不保存性能解析缓存；前端复用进行中的同 ID 请求，保留最多 24 条内存结果。活动摘要传播 `metricsRevision`，前端以 thread/turn/status/revision 识别新轮次、已结算请求和终止事件，不因工具写日志或时钟变化重算。新的结算通知到达时若旧读取仍在进行，等待旧请求结束后再读取，旧结果由请求序号丢弃；同一轮保留上次有效快照，新轮次未返回数据时显示缺失值，指标与 tips 同时替换。详情及父页导航临时保留当前数据，均不写入配置或持久化数据库。rollout 链缺失会明确报错，不用残片补算生成速度。

## 10. 验收与接手顺序

1. 先实现纯解析与聚合，使用小型合成 JSONL 验证，不依赖个人日志作为唯一测试。
2. 再只读复算第 8 节固定轮次，确认样本归属、用量总数、覆盖率与公式；差异输出原因。
3. 最后接入按需读取、状态栏、tips 与详情页，运行受影响的 Rust / 前端现有检查。

最小验收用例：

- 同一轮两次有效请求分别为 101 token / 2s、301 token / 3s，合计速度 `(100+300)/5 = 80t/s`，不是单次速度 50 与 100 的算术平均值 75。
- 另加一条缺起点的 50 token 请求：总输出 452，有效输出 402，覆盖率 `402/452`；速度仍为 80t/s，用量总数保留全部 452。
- 有效请求内部有推理和正文、结尾有工具参数：参数生成完成决定终点；后续 60 秒工具等待不改变速度。
- 只有工具参数且无起始标记、只有 0ms 推理标记、推理用量存在但无推理起点：分别排除并说明原因。
- 重复 usage、同时有 token_count 和逐请求记录、子日志继承父历史：计数不翻倍。
- 子智能体与主对话并行运行：主速度不含子用量，也不扣其整个运行时长。
- 后台命令仍在运行时主模型已继续：工具运行耗时不侵入生成速度分母。
- 坏行、半行、缺返回、时间倒退、不同模型：按第 7 节降级，没有 NaN、Infinity 或缺失值变 0。运行中/中断轮次保留此前有效请求，未结算尾段不进入速度；估算首 token 不使用正文之前的工具等待。
- 快速切换对话、旧日志无字段：显示对应 ID 的结果或 `—`，不会短暂展示上一对话的数据。

本功能的完成条件是解析、前端和真实日志核对均通过；功能实施不包含自动提交 Git 或发布版本。日志核对使用外部本地进程只读访问，用户未要求浏览器验收。

本次实现验证（2026-10-08）：

- `node tests/frontend.mjs` / `node tests/dynamic.mjs`、`npm run build` 通过；覆盖缺失值与 0、范围选择、异步竞态、子代理返回、HTML 转义、24 条缓存界限、归档单恢复菜单。
- `cargo test --manifest-path src-tauri/Cargo.toml --locked --offline`：48 项通过，8 项环境依赖检查默认忽略；`cargo fmt --check` 与 `git diff --check` 通过。
- 外部 Cargo 测试进程单独执行 `metrics::tests::local_metrics_sample -- --ignored --nocapture`：主固定轮 82/61 请求、14945 有效输出、718853ms、20.7052t/s、TTFT 2663ms；子线程 65/38、19794、635213ms、31.1014t/s。主页面直接子线程摘要亦读取成功，父子用量隔离。本组实际解析约 0.62 秒，包含主日志及直接子线程，再加一次独立子样例。
- `scripts/build-app.ps1` 已生成 `.runtime/bin/CodexChatPane.exe`，沿用 v0.2.1；未提交、发布，也未进行浏览器或原生窗口交互验收。

复算入口是忽略测试，需本机存在第 8 节日志；它只读状态库和 JSONL，不通过 Codex 内置 MCP 获取统计。当前 0ms 的推理记录没有有效起点，在请求详情中归类为“缺少输出起点”。

## 11. 参考资料

- [NVIDIA GenAI-Perf metrics](https://docs.nvidia.com/deeplearning/triton-inference-server/user-guide/docs/perf_analyzer/genai-perf/README.html#metrics)：单用户输出速度与系统吞吐量的区别。
- [Anyscale latency and throughput metrics](https://docs.anyscale.com/llm/serving/benchmarking/metrics)：TPOT 为 `(E2E - TTFT)/(输出 token - 1)`。
- [OpenAI token counting](https://developers.openai.com/api/docs/guides/token-counting)：输出用量不只包括可见正文。
- [OpenAI reasoning](https://developers.openai.com/api/docs/guides/reasoning)：推理 token 与输出用量。
- [项目接入说明](integration.zh-CN.md)、[架构说明](architecture.zh-CN.md)：现有数据来源、只读边界与刷新机制。

上述官方指标提供术语依据；第 4 节是针对当前本地日志制定的估算算法，不是 OpenAI 官方公布的 Codex 性能指标。
