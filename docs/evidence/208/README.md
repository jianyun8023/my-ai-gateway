# 来源管理与顶栏视觉修复（#208）

核对日期：2026-09-13。实现基于最新 `main` `1c7c518`，对应 [Issue #208](https://github.com/jianyun8023/my-ai-gateway/issues/208)。

## 截图问题与修复

| 截图问题 | 最终处理 |
| --- | --- |
| 搜索框、按钮和列表标题错位 | 搜索保留关联标签并提供占位提示；输入框与按钮同高，卡片标题居中对齐 |
| 三枚“原生”无法区分协议 | 同时显示 Chat / Responses / Messages 与各自模式，保留完整协议名称提示 |
| 流程提示文字粘连、配色冲突 | 标题与步骤分行，移除有色卡片，使用中性辅助文字 |
| 品牌区与顶栏底边错位 | 两侧统一为 56px |
| 辅助区域过高 | 移除重复说明，桌面顶栏显示导航分组，来源摘要使用 84px 紧凑卡片 |
| 顶栏配置拥挤、输入截断 | 桌面“网关连接”打开配置弹窗；窄屏沿用导航抽屉；语言切换改为紧凑控件 |
| 同步时间过长、待审核重复 | 日期与时间分行，完整时间仍可读取；两个位置实际使用同一 `pendingCount`，合并到模型列，仅强调非零待审核 |
| 普通信息反复使用绿色 | 普通统计与协议模式采用中性色，转换和待办仍有文字与警告色 |
| 操作、导航图标含义不清 | 行操作显示“检查更新”和刷新图标；模型与路由使用分支图标 |
| 两行表格仍有纵向滚动条 | 原因为 Switch 透明输入区域使用静态定位向下超出父容器；通过 `inset: 0` 定位到开关根节点，保留原生表格滚动规则和完整点击区域 |

## 验证

- `mise exec -- npm --prefix web run lint`：通过，包含 ESLint 和两轮 Knip。
- `mise exec -- npm --prefix web run typecheck`：通过，包含应用和测试 TypeScript。
- `mise exec -- npm --prefix web test`：36 个测试文件、263 项测试通过。
- `mise exec -- npm --prefix web run build`：通过；Vite 仍提示部分 bundle 大于 500 kB。
- 复用并更新连接与来源回归：弹窗关闭不应用草稿、返回焦点、跨断点保留草稿、应用后清空输入、身份更新，以及协议名称/模式关联和待审核计数只显示一次。
- 浏览器使用独立 Chrome、只读模拟 API 检查桌面浅色/深色、1024px 英文、390px 和 375px 窄屏；验证连接配置开关、草稿保留及表格键盘横向滚动。尺寸记录见 [layout-measurements.json](layout-measurements.json)。

本地验证范围为前端。模拟来源的协议模式仅用于覆盖展示状态，不代表任何真实 Provider 的支持情况；未执行真实 Provider 请求或生产验收。

## 视觉证据

| 场景 | 截图 |
| --- | --- |
| 1466 × 963，中文浅色 | [来源列表](sources-desktop-light.png) |
| 1466 × 963，中文深色 | [来源列表](sources-desktop-dark.png) |
| 1024 × 900，英文深色 | [来源列表](sources-tablet-en.png) |
| 390 × 844，中文浅色 | [来源列表](sources-mobile.png) |
| 375 × 812，中文浅色 | [来源列表](sources-small-mobile.png) |
| 桌面连接配置 | [配置弹窗](connection-desktop.png) |
| 窄屏连接配置 | [导航抽屉](connection-mobile.png) |

## 复现预览

在仓库根目录分别启动两个终端：

```sh
mise exec -- node docs/evidence/208/visual-fixture.mjs
```

```sh
VITE_API_PROXY_TARGET=http://127.0.0.1:8798 mise exec -- npm --prefix web run dev -- --port 5188 --strictPort
```

打开 `http://127.0.0.1:5188/#sources`。模拟 API 仅监听本机，拒绝写请求，无需真实凭据。截图使用固定的演示身份与模式数据。
