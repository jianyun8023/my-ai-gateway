# 控制台原型归档

[`ai-gateway-prototype.html`](ai-gateway-prototype.html) 是 2026-08-31 收到并评审的单文件控制台原型，当前更新版按 SHA-256 `12697c2de64d761aa1d4d2bce5da40566cc546206ea75622a35ee77b3feef78e` 原样归档，包含 920px、600px、380px 三档响应式参考。

该文件只用于产品与视觉参考，不参与 `web/` 构建，也不能作为运行时数据、领域模型或 API 契约。正式实现继续使用 React、`src/gateway-usage` adapter 和 `/admin/usage/*`。

正式设计语言维护在 [`../brand-spec.md`](../brand-spec.md)，其原始输入 SHA-256 为 `a7192512f2c5b019ad9699c35157960a7945bfbcdc7fb3d99d44fb5f7d410a7d`。评审与实施范围记录在 GitHub Issue #33，并回链产品路线图 Issue #1。正式页面实现 Overview、Analysis、Request Events 及三档响应式；来源管理、模型路由和系统设置不从该静态原型直接复制。
