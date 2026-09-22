// 跨页面状态/模式/策略枚举的展示文案(英文)——命名空间 console.values
// 数据层面仍保存原始枚举值;这里只负责「给用户看的」本地化文本。
export const values = {
  status: {
    pending: 'Pending',
    confirmed: 'Confirmed',
    unavailable: 'Unavailable',
  },
  health: {
    unknown: 'Unknown',
    healthy: 'Healthy',
    degraded: 'Degraded',
    unhealthy: 'Unhealthy',
    cooling_down: 'Cooling down',
    stale: 'Stale',
  },
  availability: {
    unknown: 'Unknown',
    available: 'Available',
    unavailable: 'Unavailable',
  },
  mode: {
    native: 'Native',
    adapter: 'Adapter',
    unsupported: 'Unsupported',
    unknown: 'Unknown',
  },
  strategy: {
    primary_then_weighted_fallback: 'Primary then weighted fallback',
  },
} as const;
