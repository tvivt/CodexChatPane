# 发布前同步记录 / Pre-release synchronization record

本文保留 2026-11-07 升级版本号之前的本地同步记录，其中版本号和未发布状态描述该时点。后续定版为 **v0.5.3**，当前版本说明见 [v0.5.3](v0.5.3.md)。

This is the historical local synchronization record before the version bump on 2026-11-07. Version and publication statements below describe that point in time. The changes were subsequently assigned to [v0.5.3](v0.5.3.md).

更新日期 / Updated: 2026-11-07

源码版本仍为 **0.5.2**。以下内容尚未发布，不包含在 GitHub 已有的 `CodexChatPane-v0.5.2.exe` 中；该附件对应的历史说明仍见 [v0.5.2](v0.5.2.md)。

## 变更

- 接入服务端重置卡历史；迟到的用卡证据补正已有重置事件的分类，保留事件 ID、周期边界和已关闭通知。
- 修复旧 `token_count` 快照与新版逐请求记录迟到时重复计入的问题。按归属、累计结算位置和用量对账，兼容压缩前后的累计偏移，不再依赖两秒时间窗口。
- 修复同毫秒的旧请求 ID 冲突，以及累计回退后重复结算位置导致请求丢失的问题；累计回退建立新计数分段，新版原生请求 ID 保持原样。
- 更新分析缓存为 `usage-analysis-v4`、用量签名为 `usage-v6`，消费数据库缓存版本为 3。旧版 0/1/2 仅重建派生消费索引与报告，保留原始额度样本、关闭记录及重置卡历史。首次打开可能需要重新解析日志。

## 验证范围

2026-11-07 本地 release 同步后复验：默认 Rust 151 passed、27 ignored，前端、动态列表、额度/预加载、Rust 格式、前端生产构建通过；两边文档文件链接及版本字段检查通过。本次仅更新两个本地检出，未提交或发布。

2026-11-06：默认 Rust 151 passed、27 ignored；前端、动态列表及额度检查通过。独立回放覆盖迟到双记录、同毫秒请求、累计回退、新旧格式混合与压缩边界；只读真实样例核对请求/Token、缓存迁移与热读零源内容。详细口径见[开发记录](../development.zh-CN.md#7-本次验证记录)和[统计说明](../conversation-metrics.zh-CN.md)。

未进行原生界面交互、真实对话写入、兑卡或真实切号；未逐条审计全部真实历史。

## English

The source keeps version **0.5.2**. These changes are unpublished and are absent from the existing GitHub v0.5.2 executable.

After local release synchronization on 2026-11-07, 151 default Rust tests passed (27 ignored), along with frontend, dynamic-list, usage/preload, formatting and frontend build checks. Documentation file links and version fields passed in both checkouts. This synchronization did not commit or publish changes.

- Reconcile server reset-card history with existing events without changing their IDs, cycle boundaries or dismissed notices.
- Reconcile delayed legacy snapshots and modern request records by ownership, cumulative endpoints and usage, including compaction offsets. Remove the two-second matching assumption.
- Keep distinct same-millisecond legacy requests and start a new counter epoch after cumulative rollback. Preserve native modern request IDs.
- Rebuild derived consumption caches with analysis version `usage-analysis-v4`, signature `usage-v6` and database cache version 3. Preserve raw quota observations, dismissals and card history; the first open may reparse logs.

On 2026-11-06, 151 default Rust tests passed (27 ignored), along with frontend, dynamic-list and usage checks. Independent synthetic and read-only real-log checks covered the accounting and migration boundaries above. Native interaction, real conversation writes, redemption, account switching and an exhaustive per-request history audit were not performed. See [development evidence](../development.md#current-unpublished-verification).
