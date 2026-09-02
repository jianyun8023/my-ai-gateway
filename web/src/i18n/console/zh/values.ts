// 跨页面状态/模式/策略枚举的展示文案(简体中文)——命名空间 console.values
// 数据层面仍保存原始枚举值;这里只负责「给用户看的」本地化文本。
export const values = {
  status: {
    pending: '待确认',
    confirmed: '已确认',
    unavailable: '不可用',
  },
  health: {
    unknown: '未知',
    healthy: '健康',
    degraded: '降级',
    unhealthy: '不健康',
    cooling_down: '冷却中',
  },
  availability: {
    unknown: '未知',
    available: '可用',
    unavailable: '不可用',
  },
  mode: {
    native: '原生',
    adapter: '转换',
  },
  strategy: {
    primary_then_weighted_fallback: '固定主选 → 加权回退',
  },
} as const;
