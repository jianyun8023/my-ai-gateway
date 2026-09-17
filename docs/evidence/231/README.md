# 主题治理与多风格预览（#231）

核对日期：2026-09-17。实现基于 `main` `7226f22`（v0.2.0）。截图使用只读合成来源数据，不代表真实 Provider 能力。

## 治理结果

- 外观拆成 `style` 与 `mode` 两个维度：风格负责品牌 token，模式负责浅色、深色与跟随系统。
- 根节点分别暴露 `data-theme-style` 和 `data-color-scheme`；Mantine、原生控件、Portal 与 Chart.js 使用同一解析结果。
- 删除没有独立视觉定义的 `white` 状态；`Theme` 类型归入唯一的主题 store，不再保留单项 `types/index.ts`。
- 页面只消费 `--bg`、`--surface`、`--fg`、`--muted`、`--border`、`--accent` 等语义 token，不按主题名称分支。
- Mantine 间距与圆角改为品牌 token；不同风格可以调整表面形态，仍共享组件契约。

## 候选风格

| 风格 | 定位 | 建议 |
| --- | --- | --- |
| 青绿控制台 / Console Green | 现有冷灰、绿色 Tech-Utility 基线 | 最稳妥，适合继续作为默认风格与回归基线 |
| 深海观测 / Deep Ocean | 蓝灰表面、青色操作信号 | **优先推荐**；与网关、路由、观测场景贴合，浅深模式辨识度和状态区分都较好 |
| 星云 / Nebula | 靛紫色、更圆润的现代 AI 风格 | 品牌辨识度最高，适合希望突出 AI 属性的产品方向 |
| 砂岩 / Sandstone | 暖灰纸面、琥珀色操作信号 | 阅读温和、最不像传统后台；强调色与 warning 色接近，若入选应再拉开两者距离 |

## 视觉证据

组合图顺序均为：左上青绿控制台、右上深海观测、左下星云、右下砂岩。

| 场景 | 截图 |
| --- | --- |
| 四套浅色风格 | [组合图](themes-light.png) |
| 四套深色风格 | [组合图](themes-dark.png) |
| 外观选择器 | [真实页面](appearance-picker.png) |

单图：[青绿浅色](utility-light.png) / [青绿深色](utility-dark.png) · [深海浅色](ocean-light.png) / [深海深色](ocean-dark.png) · [星云浅色](nebula-light.png) / [星云深色](nebula-dark.png) · [砂岩浅色](sandstone-light.png) / [砂岩深色](sandstone-dark.png)

## 复现

先启动只读视觉数据与前端：

```sh
mise exec -- node docs/evidence/208/visual-fixture.mjs
VITE_API_PROXY_TARGET=http://127.0.0.1:8798 mise exec -- npm --prefix web run dev -- --port 5188 --strictPort
```

再生成全部候选截图：

```sh
mise exec -- node docs/evidence/231/capture-theme-previews.mjs
```

## 验证

- `npm --prefix web run lint`：通过，包含 ESLint 与两轮 Knip。
- `npm --prefix web run typecheck`：通过，包含应用与测试 TypeScript。
- `npm --prefix web test`：41 个测试文件、331 项测试通过。
- `npm --prefix web run build`：通过；保留既有大于 500 kB 的 chunk 提示。
- `npm --prefix web run test:browser-smoke`：通过，覆盖错误恢复、来源草稿隔离、路由焦点与 Portal/Escape。
- 浏览器检查：四套风格均可在真实来源页面即时切换；浅深模式、Popover 可访问状态与持久化属性同步。
