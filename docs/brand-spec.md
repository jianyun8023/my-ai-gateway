# My AI Gateway — 设计语言规范

当前前端的实现映射、组件职责与维护规则见 [design.md](../design.md)。本文保留视觉方向；实现差异与本轮修正在该文档中明确记录。

## 视觉方向

**Tech-Utility**（参考 Datadog / GitHub）：高信息密度、直接展示数据、无装饰性元素。适合开发者工具和基础设施产品。

> 产品形态：单用户、自托管、界面优先的 AI Provider 聚合端控制台。

---

## 色彩系统

所有颜色使用 OKLCh 色彩空间，通过 CSS 变量 `:root` 统一管理。

### 核心色板（6 个语义 Token）

| Token | 值 | 用途 |
|---|---|---|
| `--bg` | `oklch(98% 0.005 250)` | 页面背景，冷灰蓝 |
| `--surface` | `oklch(100% 0 0)` | 卡片、侧边栏、弹窗表面 |
| `--fg` | `oklch(22% 0.02 240)` | 主文字色 |
| `--muted` | `oklch(50% 0.018 240)` | 辅助文字、标签、占位符 |
| `--border` | `oklch(90% 0.008 240)` | 分隔线、边框 |
| `--accent` | `oklch(58% 0.16 145)` | Signal green，主强调色 |

### 语义衍生色

| Token | 值 | 用途 |
|---|---|---|
| `--accent-soft` | `color-mix(in oklch, var(--accent) 14%, transparent)` | Accent 浅背景（active nav、pill） |
| `--fg-soft` | `color-mix(in oklch, var(--fg) 6%, transparent)` | Hover 背景、微弱填充 |
| `--success` | `oklch(62% 0.16 145)` | 正常 / 通过 / upstream |
| `--success-soft` | `color-mix(in oklch, var(--success) 12%, transparent)` | Success pill 背景 |
| `--warn` | `oklch(70% 0.16 80)` | 警告 / 降级 / 转换 / estimated |
| `--warn-soft` | `color-mix(in oklch, var(--warn) 12%, transparent)` | Warn pill 背景 |
| `--danger` | `oklch(58% 0.2 25)` | 错误 / 超时 / 失败 |
| `--danger-soft` | `color-mix(in oklch, var(--danger) 12%, transparent)` | Danger pill 背景 |

### 强调色使用预算

- 每个视口内 `--accent` 最多出现 **2 处**
- 典型分配：侧边栏 active 状态 + Primary CTA 或数据图表
- 分布条形图可在 accent 基础上通过 `color-mix` 产生渐变层级

### 对比度要求

- `--fg` on `--bg` / `--surface`：≥ 12:1（WCAG AAA）
- `--muted` on `--bg`：≥ 4.5:1（WCAG AA）
- `--muted` on `--surface`：≥ 4.5:1（已验证 ≈ 4.7:1）

---

## 字体系统

| 角色 | 字体栈 | 用法 |
|---|---|---|
| Display / Body | `-apple-system, BlinkMacSystemFont, 'Inter', 'Segoe UI', system-ui, sans-serif` | 标题、正文、按钮、导航 |
| Mono | `'JetBrains Mono', 'IBM Plex Mono', ui-monospace, Menlo, monospace` | 数据值、ID、URL、时间戳、表头标签、代码片段 |

> Tech-utility 方向允许 display 与 body 使用同一字族（sans-serif）。Mono 字体用于所有需要精确对齐的数值。

### 字号阶梯

| Token | 尺寸 | 用法 |
|---|---|---|
| `--fs-h1` | 28px | 页面标题 |
| `--fs-h2` | 22px | 卡片大标题 |
| `--fs-h3` | 16px | 段落标题 |
| `--fs-body` | 14px | 正文 |
| `--fs-meta` | 12px | 元数据、标签 |
| `--fs-small` | 11px | Pill 文字、微标签 |

### 数值排版

- 所有数值使用 `font-variant-numeric: tabular-nums` 保证列对齐
- `.num` class 或 `.col-num` 应用于表格数字列

---

## 间距系统

| Token | 值 | 用途 |
|---|---|---|
| `--gap-xs` | 4px | 紧凑间距（pill 内部、图标与文字间） |
| `--gap-sm` | 8px | 小间距（按钮组、筛选器间） |
| `--gap-md` | 16px | 标准间距（卡片内部、表格行间） |
| `--gap-lg` | 24px | 大间距（区块间距、page 标题下方） |
| `--gap-xl` | 32px | 特大间距（页面主区域分隔） |

### 内边距规范

- 卡片 `.card`：20px
- Topbar：0 24px（平板 16px）
- 内容区 `.content`：24px（平板 16px，手机 12px）
- Drawer body：20px（手机 14px）
- Modal body：20px 24px（平板 16px）

---

## 圆角系统

| Token | 值 | 用法 |
|---|---|---|
| `--radius` | 8px | 默认圆角（卡片，在手机端降为此值） |
| `--radius-sm` | 6px | 小圆角（按钮、输入框、Pill 内容器） |
| `--radius-lg` | 12px | 大圆角（卡片、Modal） |
| `99px` | 全圆角 | Pill 标签 |

---

## 组件规范

### 按钮

| 类型 | 样式 | 用法 |
|---|---|---|
| Primary | `--accent` 实底 + `--surface` 文字 | 每个视口 1 个主 CTA |
| Secondary | `--surface` 底 + `--border` 边框 | 次要操作 |
| Ghost | 透明底 + `--muted` 文字 | 工具栏、分页 |
| Icon | 28×28px 容器（手机 36px） | 更多菜单、关闭 |
| Danger | 透明底 + `--danger` 文字 + 30% 边框 | 删除操作 |

### Pill 标签

Mono 字体、11px、99px 圆角，5 种语义变体：

- `.pill-success`：`--success-soft` 背景 + `--success` 文字
- `.pill-warn`：`--warn-soft` 背景 + `--warn` 文字
- `.pill-danger`：`--danger-soft` 背景 + `--danger` 文字
- `.pill-muted`：`--fg-soft` 背景 + `--muted` 文字
- `.pill-accent`：`--accent-soft` 背景 + `--accent` 文字

### 数据表格 `.dt`

- 表头：mono 字体、11px、uppercase、`--muted` 颜色
- 行高：padding 10px 12px
- Hover：`--fg-soft` 背景，不改变文字颜色
- 可点击行 `.clickable-row`：`cursor: pointer`
- 移动端：wrap 在 `.table-scroll` 内横向滚动

### Drawer / Config Drawer

- 事件详情 Drawer：420px 宽
- Config Drawer：500px 宽
- 移动端：全宽 100%
- 遮罩层：30% `--fg` 半透明
- 过渡：`right 0.2s ease`

### Modal

- 默认：520px 最大宽度、85vh 最大高度
- 移动端：95% 宽、90vh 高
- 阴影：`0 20px 60px color-mix(in oklch, var(--fg) 15%, transparent)`

### 协议链路 `.proto-chain`

Mono 字体、inline-flex、节点 + 箭头链式展示：

- `.proto-node`：`--fg-soft` 背景
- Adapter 节点：`--warn-soft` 背景
- 箭头 `→`：`--muted` 颜色

### 协议能力矩阵

三种状态：

- `.cap-native`（原生）：`--success-soft/success`
- `.cap-adapter`（转换）：`--warn-soft/warn`
- `.cap-unsupported`（不支持）：`--fg-soft/muted`

---

## 交互状态

### Hover

- 导航项：`--fg-soft` 背景，文字变为 `--fg`
- 按钮（Primary）：`color-mix(in oklch, var(--accent) 85%, black)` — L 通道 -0.06~-0.12
- 按钮（Secondary）：边框变为 `--fg`
- 按钮（Ghost）：`--fg-soft` 背景 + 文字变为 `--fg`
- 表格行：`--fg-soft` 背景
- **绝不** 将文字变为 `--muted` 或更浅色

### Focus

- 所有可交互元素必须有 `:focus-visible` 焦点环
- 默认：`outline: 2px solid var(--accent); outline-offset: 2px`
- 导航项：`outline-offset: -2px`（内偏移）
- 输入框：`border-color: var(--accent); outline: 2px solid var(--accent-soft)`

### Active（导航）

- `.nav-item.active`：`--accent-soft` 背景 + `--accent` 文字 + `font-weight: 500`
- `.tab-btn.active`：`--accent` 文字 + `--accent` 下划线

---

## 响应式断点

| 断点 | 设备 | 关键变化 |
|---|---|---|
| > 920px | Desktop | 全功能，固定侧边栏 |
| ≤ 920px | Tablet | 侧边栏转为 overlay 抽屉，grid-4 → 2×2，表格横向滚动，topbar 搜索隐藏 |
| ≤ 600px | Phone | endpoint 隐藏，grid 继续塌陷，筛选器垂直堆叠，drawer 内 key-value 改为纵向排列，btn-icon 增至 36px |
| ≤ 380px | Small phone | grid-4 → 1 列，KPI 卡片改为行式 |

### 移动端侧边栏

- 汉堡按钮出现在 topbar 左侧
- 点击后侧边栏从左侧滑入（`transform: translateX`）
- 背景遮罩层 `--fg 30%` 半透明
- 导航切换后自动收起侧边栏
- `Escape` 键关闭
- `body overflow: hidden` 防止背景滚动

### 触摸目标

- 最小触摸区域：44px（手机端 btn-icon 增至 36px，实际含 padding 达到 44px）
- 导航项 padding 9px 12px + 行高确保 ≥ 44px

---

## 数据可视化

### 堆叠柱状图

- `--accent`：Input Token
- `color-mix(in oklch, var(--accent) 45%, var(--bg))`：Output Token
- 高度随数据比例缩放
- 底部标签：mono 10px

### 分布条形图

- 单行 8px 高、4px 圆角
- 颜色层级：accent → accent 70% → accent 50% → accent 35% → accent 25%
- 图例：12px 方形色块 + meta 文字

### Token 构成分布

- 12px 高度
- Input（accent）/ Output / Reasoning（warn）/ Cached
- 图例使用 `dist-legend` flex-wrap 布局

---

## 命名约定

### HTML ID

- 页面：`page-{name}` — `page-overview`、`page-providers`
- Tab 内容：`tab-{name}` — `tab-logical-models`、`tab-routes`
- 配置组件：`config-{type}` — `config-provider`、`config-model`
- Modal 表单：`modal-{type}` — `modal-provider`、`modal-key`

### `data-od-id`

所有页面区域、关键组件和重复卡片需要 `data-od-id` 属性：

- 页面级：`page-overview`、`page-providers`、`page-analysis`、`page-events`、`page-models`、`page-settings`
- 全局组件：`sidebar`、`topbar`、`event-drawer`、`config-drawer`、`modal`
- 内容区域：`kpi-row`、`token-trend`、`recent-activity`、`model-distribution`
- 表格：`providers-table`、`events-table`
- 表单：`form-new-provider`、`form-new-model`、`form-new-key`、`form-discovery-confirm`

---

## 禁止事项

- 不使用紫色渐变
- 不使用 emoji 作为功能图标
- 不使用 Inter / Roboto / Arial / Fraunces 作为 display 字体
- 不使用竖条 + 圆角卡片 callout 模式
- 不制造虚假数据，使用带标记的占位符
- Hover 状态不降低文字对比度
- 不使用 `scrollIntoView`
- 不使用 `white-space: nowrap` 强制内容溢出
- 不给每个标题都加图标
- 每个视口内不出现重复的 Primary 按钮
