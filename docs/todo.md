# 实施 TODO

- [x] 内置 `kimi-responses-adapter` workspace crate
- [x] Provider 原生协议透传（Chat/Responses/Anthropic）
- [x] Virtual Key 创建、列表、撤销、模型白名单
- [x] 非流式 usage 提取和 PostgreSQL 基础落库
- [x] Kimi Adapter 非流式/流式 mock 回归测试
- [x] PostgreSQL 控制面表 migration（providers/accounts/routes）
- [x] 账号健康冷却的内存实现
- [x] Usage summary/events 管理 API
- [ ] 流式 SSE 末事件 usage 自动落库
- [x] 非流式 usage 缺失时的基础字节估算
- [ ] Provider/Account/Route CRUD 与数据库加载
- [ ] Virtual Key 轮换与分组权限
- [ ] Keeper React UI 集成（Usage/Analysis/Events）
- [ ] Usage Analysis 聚合 API（时间、模型、Provider、账号、Key）
- [x] 账号健康冷却的内存实现
- [ ] 健康状态持久化、重试次数统计和完整加权 fallback
- [ ] OTel/Prometheus、凭据加密、审计和 SSRF allowlist
