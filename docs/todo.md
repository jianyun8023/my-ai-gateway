# 待办与验收

2026-09-09 核对。实现以源码和已合并 PR 为准，任务状态以 GitHub 为准；历史测试记录见 [验收清单](v0.1.0-acceptance.md)。

## 当前开放任务

| 任务 | 已有结果 | 剩余工作 |
| --- | --- | --- |
| [#113 测试总计划](https://github.com/jianyun8023/my-ai-gateway/issues/113) | Contract、SDK、差分框架与本地外部扫描已接入 | 补齐 Kimi 原生切换后的真实 Provider 覆盖与生产复验，再核对总体验收 |
| [#120 性能与故障验证](https://github.com/jianyun8023/my-ai-gateway/issues/120) | [PR #159](https://github.com/jianyun8023/my-ai-gateway/pull/159) 已合并故障测试、压测工具和 Mock 性能基线 | 明确含 PostgreSQL 写入、实际网络与部署资源的性能验收范围 |

## 发布前

1. 在部署环境确认 migration（含 #110 的 `0024_system_events.sql`）、Provider preset 和 Kimi 原生 Responses 行为，按 [live smoke](live-provider-smoke.md) 执行真实 Provider 验证。
2. 补齐 #96/#97/#98 的生产数据证据，尤其是 #98 的 estimated usage 根因。它们已关闭，但现有验收记录未完成生产复验。
3. 验证生产 OTel collector 接收、数据保留与恢复流程（含 `system_events`），以及历史生产数据规模下的 `/admin/events` 查询性能；完成验收后再创建版本 tag / Release。

PR #159 的本地结果包括 24 组 Mock 性能矩阵，报告见 [性能基线](../tests/load/baselines/2026-09-07-m3-max/README.md)。这些结果不含 PostgreSQL 用量写入和生产网络开销。

## 已完成实现（生产验收另行跟踪）

[#110 事件中心](https://github.com/jianyun8023/my-ai-gateway/issues/110) 已按方案 C 由 [PR #183](https://github.com/jianyun8023/my-ai-gateway/pull/183) 合并实现（main 合并提交 `e3c9e24`）：新增窄 `system_events`、统一查询 API、恢复语义矩阵和“运行事件”页。最终精确 head 的独立评审与 CI 已通过，7/7 项代码验收完成，Issue 已关闭。

上述结论不包含真实 Provider / live Codex E2E、生产部署后的 migration / retention，或历史生产数据规模下的查询性能；这些边界继续按上方发布前项目及 #113 / #120 跟踪。

## 已完成的控制台治理（历史证据）

[#166](https://github.com/jianyun8023/my-ai-gateway/issues/166) 的 54 项原始范围与验收项已全部完成并关闭；PR [#169](https://github.com/jianyun8023/my-ai-gateway/pull/169)–[#182](https://github.com/jianyun8023/my-ai-gateway/pull/182) 已合并。逐批范围、代表性截图、最终八页面业务状态映射和未外推的环境边界见 [Mantine 迁移清单](mantine-migration.md#对-166-原始未勾项的最终映射)；当前组件与页面约束以 [design.md](../design.md) 和 [前端架构](frontend-architecture.md) 为准。

以下是 2026-09-07、[#160](https://github.com/jianyun8023/my-ai-gateway/issues/160) 对应批次的历史实现与本地验证记录，不是当前待办：

- 移除顶栏搜索、`⌘K`、静态运行状态和绿色状态点；保留页面现有筛选与网关地址展示。
- 接入 Virtual Key 轮换，支持 0–86400 秒重叠期、新密钥模型白名单、显式查看/复制、旧密钥截止时间与状态刷新。
- 新增 13 项轮换回归：白名单变更/清空、重叠期边界、失败重试、重复提交保护、不可轮换状态与到期更新。前端 116 项测试、lint、类型检查和构建通过；使用本地模拟 API 完成浏览器轮换展示与英文桌面、390px 窄屏表单检查。

以上历史证据不代表 PostgreSQL 鉴权、真实 Provider、整条接入调用流程或生产复验完成；这些边界继续按上方发布前任务推进。#110 后续新增的第九个“运行事件”入口有独立测试与浏览器记录，不能由 #166 的八页证据推断。
