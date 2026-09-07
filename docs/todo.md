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

- 核对真实交互：顶栏搜索与运行状态目前是静态展示，应接入功能或移除占位。
- 补齐 Virtual Key 轮换入口，支持设置重叠期并调整模型白名单，复用已有轮换 API。
- 以“接入来源 → 模型发现/确认 → 绑定 → 路由 → 客户端调用”为主线，完成中英文、窄屏和键盘验收。
