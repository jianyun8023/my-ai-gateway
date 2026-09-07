# 实施 TODO

> 状态基线：`main c4da717` + `codex/113-release-test-closeout` 本地收尾（2026-09-07 更新，未推送）。实现事实以当前代码和已合并 PR 为准；开放任务以 GitHub Issue 为准。

## 已完成

- [x] 三类北向协议入口与 Provider 原生 JSON/SSE 透传
- [x] 内置 `kimi-responses-adapter` 及请求、非流式、流式回归（已于 #157 移除：Kimi Code 官方原生支持 Responses，迁移至 `kimi_code@4` native）
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
- [x] 首选账号固定优先、账号级模型重写、跨 Source/Provider native fallback、统一加权选择和持久化健康冷却/探测
- [x] 展示 DB-first 三协议有效能力矩阵（#43；关闭 #24）
- [x] 完成 Source 接入、模型发现差异和确认流（#45；关闭 #6）
- [x] 健康状态持久化与主动探测（#52）
- [x] SSE 心跳、取消和流式超时契约（#54）
- [x] 完整落地 AI Gateway 生产控制台原型（#60）
- [x] Token 时序、模型与来源分布为核心的用量分析（#8）
- [x] Kimi 非流式 Adapter Usage 入库、ProviderPreset 能力版本和 opt-in Live Smoke（#62；PR #72）
- [x] 统一 Secret Resolver 与凭据信封加密（#47；PR #73）——AES-256-GCM 信封加密、多版本 keyring、环境/密文/内联三选一、运行时凭据路径和 Admin 加密/轮换端点已集成
- [x] Admin 写操作审计日志（#48；PR #74）——请求级 AuditContext、diff 脱敏、事务内记录和 fallback 独立记录，中间件已接入所有 Admin 路由
- [x] Virtual Key 轮换、权限更新和静态 Key 迁移（#51；PR #75）——rotate/revoke/scopes 端点、DB 层事务和 migration 0015 已集成
- [x] Prometheus 指标采集与 `/metrics` 端点（#50；PR #77）——请求/attempt/Token/延迟/TTFT/冷却/snapshot/活跃流指标已接入非流式和流式路径

- [x] OpenTelemetry OTLP/gRPC tracing 导出（#50；PR #80），由 `OTEL_EXPORTER_OTLP_ENDPOINT` 显式启用

## 当前开放任务

- [ ] #113 测试总计划：本地收尾包含 Contract、外部扫描、SDK、故障与性能基线；GitHub 状态尚未回写。
- [ ] #120 性能/故障验证：本地工具与测试已实现，运行结果见验收清单及 `tests/load/`；真实 Provider 性能未执行。
- [ ] #110 运行时事件中心：设计讨论，不属于本轮发布前测试。

### 验收与收口

- [x] v0.1.0 验收执行：完整分步清单见 [v0.1.0-acceptance.md](v0.1.0-acceptance.md)——已有历史执行记录；本轮新增验证单独记录，不把历史勾选视为当前版本全覆盖
- [ ] 生产数据复验 #96/#97/#98 [需要人工]：三个 Issue 虽已关闭，生产修复/观测证据仍未补齐；#98 仅完成诊断，不能认定已定位根因。
- [x] 浏览器验收 #43/#45/#52/#54/#60 相关的 Web 页面功能——2026-09-06 浏览器自动化验收通过，过程缺口已修复（#151/#153/#155，PR #152/#154/#156）
- [x] 真实 Provider 联调复验（历史 2026-09-06、Kimi 原生切换之前）——低成本 live smoke 5/5 passed；不等于三家三协议全覆盖，当前版本仍需显式复验
- [ ] 打 v0.1.0 Release tag

## Epic 状态

- #2 已关闭：所有子任务完成（#12、#13、#14、#24、#6、#43、#45）。
- #6 已关闭：后端模型目录和发现链 + Web UI (#45) 全部完成。
- #8 已关闭：Usage 三主导航 + Provider/Source 归因 + 验收证据。
- #24 已关闭：后端 API + Web 有效能力矩阵 (#43) 完成。
- #1、#50 已关闭；代码完成与生产验收状态分开记录。

本文件的主线状态同步和中文 README 修正由 #55 完成。
