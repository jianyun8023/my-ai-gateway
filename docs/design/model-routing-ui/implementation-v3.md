# 模型与路由 V3 实施

依据 [Issue #195 的最终 V3](https://github.com/jianyun8023/my-ai-gateway/issues/195#issuecomment-5642790499)，页面仅保留模型列表与一个编辑抽屉。V1 / V2 SVG 留作设计历史，不作为本轮实现范围。

## 组件清单

先确定组件复用关系，再组合页面；沿用 [现有设计规范](../../../design.md)。

| 界面内容 | 组件 |
| --- | --- |
| 模型列表、局部横向滚动 | 现有 `Card`、Mantine `Table`、`TableScroll` |
| 搜索、新增、编辑 | 现有 `TextField`、`Button`、`IconButton` |
| 主备线路、协议、健康状态 | 现有 `Card`、`StatusPill`、`ProtocolPill` 与领域布局 |
| 编辑抽屉及底部操作 | 现有 `Modal` 的 drawer 模式、`FormActions`、`useOverlayState` |
| 名称、启用、来源账号、上游模型、超时与重试 | 现有 `TextField`、`SelectField`、`Toggle`、`FormGrid`；表格开关可隐藏重复状态文字，保留可访问名称 |
| 线路排序 | 现有按钮提供上移、下移和移除；拖动复用同一排序操作 |
| 高级信息 | Mantine `Accordion` + 现有 `DetailList` / `DetailItem` |
| 加载、失败、空态、保存成功 | 现有 `LoadingState`、`ErrorState`、`FormError`、`EmptyTable`、通知 |

本轮仅新增 Accordion 的按需样式与统一主题映射。折叠、键盘和可访问语义交给 Mantine。其他新文件只承担模型配置、数据聚合与页面布局，不建立另一套基础控件。

背景、文字、边框使用 `--bg` / `--surface` / `--fg` / `--muted` / `--border`。主操作使用 `--accent`；健康、部分不可用和失败分别使用既有 success / warning / danger 状态。备用身份以文字与顺序表达，不照搬原型蓝色，也不以颜色替代状态说明。

## 行为

每个逻辑模型仅占一个主要条目。协议从已发布能力读取，线路按来源、账号、上游模型合并。协议之间路径不同时，在同一条目中分别说明；未知与不可路由不能显示为支持。

编辑抽屉维护一个有序线路数组，服务端在一个事务内验证并写入模型、Binding 和 Route，然后发布运行时快照。新配置采用真实顺序回退。现有加权配置读取时仍如实说明备用选择方式，保存编辑后切换为所列顺序；不把旧加权行为画成确定的备用顺序。

协议默认只读，高级信息默认折叠。V3 不提供 Binding / RouteRule 一级管理页、路由策略选择、Route Builder、Model Fallback、Capability Filter、发布草稿或额外模型装饰字段。

## 验证记录

2026-09-12，本地工作树基于 main `848f581`。

| 检查 | 结果 |
| --- | --- |
| Web TypeScript | 应用与测试检查通过 |
| Web ESLint / Knip | 全量及生产入口检查通过 |
| Web Vitest | 36 个文件、231 项通过 |
| Web 生产构建 | 通过；仍有既有主 chunk 超过 500 kB 的体积提示 |
| Rust 静态检查 | `cargo check`、Clippy（包含 test-support）、fmt check 通过 |
| Rust / Contract / Mock | 242 + 90 + 35，共 367 项通过，0 失败、0 跳过；独立 PostgreSQL 测试库，`--include-ignored` 实际执行所有数据库集成测试 |
| JSON 配置 / diff | `mise run config-check`、`git diff --check` 通过 |
| 浏览器 | 实际生产构建 + 本地合成 Admin API；1440×900 / 1280×800 桌面、390×844 窄屏，中英文与浅深主题；局部表格滚动、抽屉全宽，document 宽度与视口相等 |
| 编辑交互 | 按钮排序后焦点留在对应线路；保存后主备变更出现在列表、焦点返回编辑入口；高级信息展开后没有可编辑字段；Select 的第一次 Escape 关闭选项，第二次关闭抽屉；浏览器无 warning/error |

模型聚合回归覆盖不同协议主备分歧、旧加权备用池、冷却/停用/未知健康、原生与转换并存。编辑回归覆盖添加/移除/拖动、单次原子提交、失败草稿、查询取消、必填与数值校验、停用模型空线路保存。

后端回归覆盖三协议实际顺序回退、重试与冷却跳过、总超时和 SSE 不重放、事务回滚与模型状态恢复、迁移及聚合接口、PostgreSQL Usage / attempt 去重与归因。顺序回退的客户端取消沿用既有流处理与结算链，本轮未新增该分支的专用取消用例。

浏览器使用合成数据，没有发送真实模型请求。本轮不将本地界面和数据库回归外推为生产部署或真实 Provider 验收。
