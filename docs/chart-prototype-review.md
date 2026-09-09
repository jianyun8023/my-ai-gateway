# 用量图表与原型核对（#166，第五批）

2026-09-08；实现基线为 `main ab5dad0`（PR #172）。原型依据是 [归档说明](prototypes/README.md)指定的 [2026-08-31 HTML 原型](prototypes/ai-gateway-prototype.html)，重点核对总览与用量分析。原型用于交互和视觉参考，数值为示意；当前 API、Token 语义和领域约束仍是实际行为依据。本文是 #166 第五批历史快照；其中“未增加事件中心”仅描述该批范围，#110 后续已另行实现运行事件入口。

## 差距与本批处理

| 项目 | 原型 | 迁移前 | 本批结果及保留差异 |
| --- | --- | --- | --- |
| 总览趋势 | 140px Input/Output 堆叠柱；实际渲染为绿色/青色 | 310px 双轴折线，默认 Total Token | 默认增加真实 Input/Output 堆叠柱；240px 保留时间轴、数值轴及图例空间。其他六项指标仍可切换折线；双轴标注各自单位。品牌 accent 与 muted 区分两条序列，未逐像素复制原型配色 |
| 图表主题 | 随原型主题变化的 CSS 色值 | canvas 固定颜色，轴与 tooltip 使用 Chart.js 默认色 | 读取统一品牌 Token；图例、坐标轴、网格、tooltip 与数据序列随既有主题状态变化。关闭插值动画；未引入第二份主题存储 |
| 分布密度 | Provider/协议为紧凑名称、百分比与细条 | 八张固定 280px 横条 canvas；tooltip 按 `parsed.y` 读取横图，可能显示类别索引 | 共享 Mantine Progress 行，直接显示 Token、占全范围比例和逻辑请求数。移除横图 tooltip 路径；按 Token 排序，显示前 N/总组数；零值不再有最少 4% 的假条形 |
| 数值与可访问性 | 静态示意柱，无精确数据操作 | 趋势值主要依赖 hover | 趋势可展开 Mantine Table，提供完整整数与时间；图表具有可访问名称。分布数值与名称留在 DOM，进度条提供数值描述 |
| Token 构成 | 四类 Token 拼成 100% 堆叠 | 独立进度条，零类被隐藏，未说明重叠 | 合并成一张 Input/Output 分段图，按两者合计计算占比；推理、缓存读取、缓存创建改为紧凑数值明细，保留零值。上报 Total 独立保留；合计不一致时展示说明。不把可能重叠或口径不同的字段相加 |
| 来源延迟 | Source、P50/P90/P99、请求数与 Usage Source 表 | 不同维度混在同一平均延迟横图中 | 只按 Source 显示平均值、真实 P95、请求数；缺失为 `—`，真实零值为 `0 ms`。P95 是已有后端字段，本批补前端映射；P50/P90/P99 和来源内 usage-source 构成仍缺接口支持 |
| 总览卡片布局 | 趋势右侧为运维动态，下方整宽模型请求分布 | 趋势右侧 Token 构成，下方最近请求与模型 Token 分布 | 本批保留 Token 优先布局；模型分布仍按 Token 衡量。最近请求不是健康/配置/路由运维事件中心，不冒充原型的运维动态（参见 #110） |
| KPI 环比 | 较昨日涨跌 | 请求/attempt、失败数、最终用量、全范围 P95 | 保留现有真实辅助信息；当前查询没有前一对比窗口，不用示意涨跌补齐 |
| 分析维度数量 | 重点呈现 Provider/入口协议 | 八个领域维度 | 保留逻辑/上游模型、Provider、Source、客户端来源、账号、入口/上游协议；维度多于原型是现有产品能力，本批只统一展示组件 |

## 数据依据与验证边界

- 数据链：`src/infra/db/usage.rs::usage_breakdown` 查询已有 `average_latency_ms` 和 `p95_latency_ms`；前端在 `web/src/gateway-usage/adapter.ts` 映射为可选值，再由 `UsageAnalysis.tsx` 展示。没有新增后端接口或 migration。
- 总览趋势 `UsageTrend.tsx` 使用 API 的各时间桶；切换 Total 时读取 `tokens.total`。回归样例 Input 1200、Output 420、Total 1780，用于验证未强制将 Total 改为 1620。
- 分布分母是同一筛选范围的 summary 总 Token，截取前 N 行前不重新归一化；总量为零时比例显示 `—`。每条分布仍保留实际请求数。
- 构成图的分母单独定义为 Input + Output；没有输入/输出时展示中性轨道与无数据说明。缓存和推理不进入图中分母。不同协议的 input 是否包括缓存并不统一（`extract_json` 保留上游字段），原来的简单比值现明确标为“缓存读取 / 输入”，不再命名为缓存命中率。
- 统计窗口、粒度、归因与导出路径保留。未增加成本核算或运维事件中心，也未将 unknown/missing 解释为健康或无消费。
- API/框架文档查询先尝试 Context7，查询失败后参考 [Chart.js 堆叠柱与轴说明](https://www.chartjs.org/docs/latest/charts/bar.html)及 [tooltip 文档](https://www.chartjs.org/docs/latest/configuration/tooltip.html)，并核对本地安装类型。

本批的 DOM/适配器回归覆盖堆叠与独立总量、切换指标/双轴、展开精确表、分布排序/截取/分母、零值、单张构成图及缓存/推理不参与归一化，以及来源延迟/P95 缺失与真实零值。浏览器已经核对浅深色、390px、数据点提示、数值表和单图构成；完整结果见[迁移清单](mantine-migration.md)第五批。

视觉依据：[原型总览](evidence/166/b5-prototype-overview.png)、[原型分析](evidence/166/b5-prototype-analysis.png)、[实现总览](evidence/166/b5-overview-light.png)、[实现分析](evidence/166/b5-analysis-light.png)、[单图构成](evidence/166/b5-composition-light.png)。原型含静态示意数据，实现截图均使用合成数据；不将两者的数值用于数据正确性比较。
