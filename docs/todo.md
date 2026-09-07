# 待办与验收

2026-09-07 核对。实现以源码和已合并 PR 为准，任务状态以 GitHub 为准；历史测试记录见 [验收清单](v0.1.0-acceptance.md)。

## 当前开放任务

| 任务 | 已有结果 | 剩余工作 |
| --- | --- | --- |
| [#113 测试总计划](https://github.com/jianyun8023/my-ai-gateway/issues/113) | Contract、SDK、差分框架与本地外部扫描已接入 | 补齐 Kimi 原生切换后的真实 Provider 覆盖与生产复验，再核对总体验收 |
| [#120 性能与故障验证](https://github.com/jianyun8023/my-ai-gateway/issues/120) | [PR #159](https://github.com/jianyun8023/my-ai-gateway/pull/159) 已合并故障测试、压测工具和 Mock 性能基线 | 明确含 PostgreSQL 写入、实际网络与部署资源的性能验收范围 |
| [#110 事件中心](https://github.com/jianyun8023/my-ai-gateway/issues/110) | 设计讨论 | 确定记录/查询方式、恢复语义及导航入口后再实施 |

## 发布前

1. 在部署环境确认 migration、Provider preset 和 Kimi 原生 Responses 行为，按 [live smoke](live-provider-smoke.md) 执行真实 Provider 验证。
2. 补齐 #96/#97/#98 的生产数据证据，尤其是 #98 的 estimated usage 根因。它们已关闭，但现有验收记录未完成生产复验。
3. 验证生产 OTel collector 接收、数据保留与恢复流程；完成验收后再创建版本 tag / Release。

PR #159 的本地结果包括 24 组 Mock 性能矩阵，报告见 [性能基线](../tests/load/baselines/2026-09-07-m3-max/README.md)。这些结果不含 PostgreSQL 用量写入和生产网络开销。

## 控制台后续

- 以“接入来源 → 模型发现/确认 → 绑定 → 路由 → 客户端调用”为主线，完成中英文、窄屏和键盘验收。

2026-09-07 本地分支 `codex/ui-design-components` 已完成（尚未推送/合并）：

- 移除顶栏搜索、`⌘K`、静态运行状态和绿色状态点；保留页面现有筛选与网关地址展示。
- 接入 Virtual Key 轮换，支持 0–86400 秒重叠期、新密钥模型白名单、显式查看/复制、旧密钥截止时间与状态刷新。
- 新增 13 项轮换回归：白名单变更/清空、重叠期边界、失败重试、重复提交保护、不可轮换状态与到期更新。前端 116 项测试、lint、类型检查和构建通过；使用本地模拟 API 完成浏览器轮换展示与英文桌面、390px 窄屏表单检查。

以上不代表 PostgreSQL 鉴权、真实 Provider、整条接入调用流程或生产复验完成；这些验收继续按上方任务推进。
