# 实施 TODO

> 状态基线：`main@bd4d689`（2026-08-31，已包含 PR #64）。实现事实以当前代码和已合并 PR 为准；开放任务以 GitHub Issue 为准。

## 已完成

- [x] 三类北向协议入口与 Provider 原生 JSON/SSE 透传
- [x] 内置 `kimi-responses-adapter` 及请求、非流式、流式回归
- [x] Source/Account/模型级协议能力矩阵、Adapter 注册表和完整路由链（#3、#4、#5）
- [x] ProviderPreset、ModelPreset、模型发现差异和用户确认 API（#12、#13）
- [x] PostgreSQL DB-first 启动、一次性 JSON 初始化/显式导入和原子 runtime snapshot（#14）
- [x] Source、Account、LogicalModel、ModelBinding、Route 事务化 CRUD 与启停（#14）
- [x] confirmed/available Binding resolver 与 `/v1/models` 可路由/健康过滤（#14）
- [x] DB runtime 有效能力矩阵 API：完整转换链、primary/fallback、degraded 和结构化不可路由状态（#24 后端范围）
- [x] PostgreSQL-backed Virtual Key 创建、列表、撤销和模型白名单
- [x] Usage v1 查询、组合筛选、确定性分页、attempt 明细和受限导出（#15、#27、#28）
- [x] 失败请求不估算 Token，`usage_source` 契约统一（#25）
- [x] Usage 记录 `route_id`、`streamed`、脱敏 `error_summary`、真实 `upstream_model_id` 和流式 `ttft_ms`（#26）
- [x] 运行时 `source_id` 与客户端自报 `client_source` 分离，跨 Source fallback 可逐 attempt 审计（#30）
- [x] 网关原生 Overview、Analysis、Request Events 用量控制台及响应式基线（#16、#29、#33）
- [x] 首选账号固定优先、账号级模型重写、跨 Source/Provider native fallback、统一加权选择和内存健康冷却

## 当前开放任务

### 管理端

- [x] 建立控制面管理 UI 外壳与独立 Management 空间（#42）
- [ ] 展示 DB-first 三协议有效能力矩阵（#43；#24 的剩余范围）
- [ ] 完成 Source 接入、模型发现差异和确认流（#45；#6 的剩余范围）

### 用量与运行时

- [x] 将 Usage 的 Provider 归因与 Source 维度真正解耦（#56；ProviderPreset 与 Source 分列）
- [ ] Virtual Key 轮换、权限更新和静态 Key 迁移（#51）
- [ ] 健康状态持久化与主动探测（#52）
- [ ] SSE 心跳、取消和流式超时契约（#54）

### 安全、质量与运维

- [x] Admin API fail closed，并与下游鉴权完全分离（#44）
- [x] Provider URL allowlist 与 SSRF 防护（#46）
- [ ] 统一 Secret Resolver 与凭据信封加密（#47）
- [ ] Admin 写操作审计日志（#48）
- [x] GitHub Actions 全量验证门禁（#49）
- [ ] Prometheus 与 OpenTelemetry（#50）
- [x] 数据保留、清理、备份与恢复（#53；运维审计、脱敏控制面导出和 PostgreSQL Runbook）

## Epic 状态

- #2 继续等待 #24 的 Web 范围（#43）收口。
- #6 的后端模型目录和发现链已完成，继续等待 #45。
- #8 的 Provider/Source 独立归因已补齐，等待最终验收关闭。
- #24 的后端聚合 API 和 Management 外壳已完成，继续等待 #43。
- #1 继续作为产品路线图保留。

本文件的主线状态同步和中文 README 修正由 #55 完成。
