# 上游账号额度监控设计

关联：[#221](https://github.com/jianyun8023/my-ai-gateway/issues/221) / [PR #222](https://github.com/jianyun8023/my-ai-gateway/pull/222)

## 1. 目标与边界

现有 Gateway Usage 回答“网关已经产生了多少请求 / Token / 延迟”；上游额度监控回答“某个上游账号现在还能使用多少 Provider 侧资源”。两类数据不能混为同一个统计口径。

V1 只提供受 Admin 鉴权保护的**只读运维观测**：

- 窗口型额度：5 小时、7 天及 Provider 返回的其他窗口；
- 余额型资源：例如 DeepSeek 账户余额；
- 刷新状态、认证错误、不支持与停用状态；
- 单账号刷新与批量刷新；
- Provider 原始响应仅在账号详情中用于排障，不进入列表/批量响应或普通日志。

明确不包含：充值、账单支付、余额修改、额度扣减/修改、自动购买、风控规避，也不让额度查询参与代理请求热路径。

## 2. 分层

```text
Admin UI
  ↓
/admin/upstream-quotas*
  ↓
upstream_quota control-plane read service
  ├─ Account / Source metadata from PostgreSQL
  ├─ SecretResolver（只在服务端解析账号凭据）
  └─ SourceHttpClient / SourceUrlPolicy
       ↓
       Provider read-only quota endpoint
       ↓
       normalized QuotaResource[]
```

额度请求与模型代理请求完全隔离。Provider 额度接口失败时，仅返回额度状态错误，不改变 Account health、路由权重或 fallback 行为。

## 3. 统一资源模型

API 不为具体 Provider 写死 `five_hour_remaining` / `seven_day_remaining` 字段，而返回资源数组：

```json
{
  "account": {
    "account_id": "kimi-main",
    "source_id": "kimi-cn",
    "provider_id": "kimi_code"
  },
  "status": "ok",
  "resources": [
    {
      "type": "window",
      "key": "5h",
      "label": "5 小时",
      "unit": "percent",
      "used": 27,
      "remaining": 73,
      "limit": 100,
      "reset_at": "2026-09-16T12:24:00Z"
    }
  ],
  "fetched_at": "2026-09-16T10:42:18Z",
  "attempted_at": "2026-09-16T10:42:18Z",
  "latency_ms": 382,
  "stale": false,
  "refresh_error": null
}
```

资源类型：

- `window`：时间窗口配额；V1 统一把可换算的窗口表示为 `percent`。
- `balance`：货币或 Provider 自定义余额；`unit` 保存币种/单位。

状态：

- `ok`
- `low`：任一百分比窗口剩余 `< 20%`
- `exhausted`：任一百分比窗口剩余 `<= 0%`，或 Provider 明确报告不可用
- `refresh_failed`
- `auth_error`
- `unsupported`
- `disabled`

UI 的 `refreshing` 是客户端瞬时状态，不要求 Provider 返回。

## 4. Admin API

所有端点沿用 `GATEWAY_ADMIN_KEY` 鉴权与 Admin 审计中间件：

| 方法 | 路径 | 语义 |
| --- | --- | --- |
| `GET` | `/admin/upstream-quotas` | 查询所有账号当前额度；不返回 Provider raw payload |
| `POST` | `/admin/upstream-quotas/refresh` | 显式批量刷新；单账号失败不阻塞其他账号；不返回 raw payload |
| `GET` | `/admin/upstream-quotas/{account_id}` | 查询一个账号；可返回 raw payload 供 Admin 排障 |
| `POST` | `/admin/upstream-quotas/{account_id}/refresh` | 显式刷新一个账号；可返回 raw payload |

批量结果始终按账号返回独立状态；Provider 401/403 映射为 `auth_error`，网络/协议/解析错误映射为 `refresh_failed`。普通 Provider 错误不升级为整个批量 API 的 5xx。

### V1 stale-while-refresh

当前实现把最近成功的**规范化资源快照**保存在当前浏览器标签页的 `sessionStorage`，按 `account_id` 隔离；不会缓存 Provider raw payload。组件卸载、列表/详情切换以及页面 reload 后仍可恢复该会话快照。

当刷新返回 `refresh_failed` / `auth_error` 且本次没有有效资源时，客户端继续展示最近成功资源，同时保留本次失败状态、错误与 `attempted_at`，并把记录标记为 `stale`。

UI 会把 `stale` 的额度资源明确标为历史快照：它不计入“最新额度”或当前额度告警。只有本次成功额度快照的窗口资源可说明具体窗口已耗尽；全局 `exhausted` 若没有相应窗口资源，只能显示为 Provider 明确报告不可用。HTTP 403 只是额度查询被拒绝，不能单独推断额度耗尽。

额度页以一次只读 `GET /admin/health` 显示已有的路由健康事实（含其自身的 `stale`、记录时间和已脱敏的最近健康失败），不读取 `available` 来宣称账号已验证健康，也不触发探测、自动恢复冷却或改变路由。额度快照和健康快照各自判定新鲜度，互不覆盖。近期请求异常不在额度快照契约中；页面只链接至同一 Source 的既有运行事件视图，避免把未经关联的失败归因给账号额度。

服务端持久化快照、跨浏览器共享 TTL 缓存和持久化刷新历史不属于当前首批实现；如后续加入，必须使用独立事实表/缓存，不写入 `usage_events`，也不能改变代理请求热路径。

## 5. Provider V1 映射

### DeepSeek

- Endpoint：`GET {source.base_url}/user/balance`
- 使用 Source / Account 现有鉴权配置。
- `balance_infos[]` 转成一个或多个 `balance` 资源。
- `is_available=false` 映射为 `exhausted`。

### Kimi Code

- Endpoint：`GET {source.base_url}/v1/usages`，Kimi Code CN 默认即 `https://api.kimi.com/coding/v1/usages`。
- 当前响应按顶层 `usage` + `limits[]` 解析：`usage` 映射周窗口（7d）；`limits[]` 中 `window.duration=300` 且 `timeUnit=TIME_UNIT_MINUTE` 的项映射 5h。
- 每个 `detail`/`usage` 使用 `limit / used / remaining / resetTime`；额度统一换算为百分比，`resetTime` 保留为 UTC 时间。
- 如果接口可访问但没有已知窗口，返回 `unsupported`，不伪造 0%。

### MiniMax Token Plan

- Global：`https://www.minimax.io/v1/token_plan/remains`
- CN Source：`https://www.minimaxi.com/v1/token_plan/remains`
- 优先读取 `current_interval_remaining_percent` / `current_weekly_remaining_percent`；缺失时尝试由 usage/total count 换算。
- 重置时间优先读取 `end_time / weekly_end_time`（epoch，当前响应为毫秒）；缺失时把 `remains_time / weekly_remains_time` 明确按**毫秒倒计时**加到当前时间。
- `status_code=2062` 视为当前账号/套餐不支持 Token Plan 查询。

### Custom Provider

V1 返回 `unsupported`。后续接入必须增加明确的 Provider quota adapter / contract，不允许根据 URL 或响应字段猜测。

## 6. 安全约束

- 所有 Provider URL 继续经过 `SourceHttpClient` / `SourceUrlPolicy`，沿用 allowlist、DNS、重定向与 SSRF 边界。
- 账号凭据只通过 `SecretResolver` 在服务端解析，不进入响应、日志或原始 Provider 数据。
- Provider 响应限制为 1 MiB，并设置独立 10 秒超时；无 `Content-Length`/chunked 响应按流式 chunk 累计，超过 1 MiB 立即终止读取，不先完整缓冲。
- 原始响应只返回给 Admin 单账号详情/单账号刷新；列表和 refresh-all 不携带 `raw` 字段，日志仍禁止记录 Authorization、API Key 和完整响应正文。
- 浏览器 `sessionStorage` 只保存规范化资源，不保存 raw payload。
- 额度接口不得影响 Account health、路由权重、冷却或 fallback。

## 7. UI

一级导航放在「监控」下，独立于 Gateway Usage 的时间筛选页面：

```text
监控
├── 总览
├── 用量分析
├── 请求事件
├── 上游额度
└── 运行事件
```

路由：

- `#upstream-quotas`
- `#upstream-quotas/<accountId>`

原型：

- `docs/ui/upstream-quota/upstream-quota-list.svg`
- `docs/ui/upstream-quota/upstream-quota-refresh.svg`
- `docs/ui/upstream-quota/upstream-quota-kimi-detail.svg`
- `docs/ui/upstream-quota/upstream-quota-deepseek-detail.svg`

## 8. 验证边界

普通 PR CI 不使用真实 Provider Key，因此能验证：

- 当前 Kimi `usage + limits[]` JSON 形状归一化单测；
- MiniMax epoch/millisecond reset 语义单测；
- 列表 raw projection 与 1 MiB 流式硬上限辅助回归；
- 浏览器会话级 SWR 缓存回归；
- Admin Client / 路由契约；
- Rust/Web 静态检查与构建；
- 现有 PostgreSQL 回归没有被破坏。

真实 Kimi / MiniMax / DeepSeek 额度响应仍需在具备测试账号时做显式 live 验收，不能由 Mock/fixture 结果推断生产可用。
