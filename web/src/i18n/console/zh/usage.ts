// 网关用量三页文案(简体中文)——命名空间 console.usage
// 覆盖 Overview / Analysis / Request Events 三个标签页(单一 GatewayUsagePage)。
export const usage = {
  // —— 字段名(筛选、维度卡、事件表列、详情共用)——
  field: {
    time: '时间',
    logical_model: '逻辑模型',
    upstream_model: '上游模型',
    provider: '提供商',
    source_id: '来源 ID',
    source_account: '来源 / 账号',
    client_source: '客户端来源',
    account: '账号',
    protocol: '协议',
    protocol_in: '入站协议',
    protocol_upstream: '上游协议',
    virtual_key_id: '虚拟密钥 ID',
    status: '状态',
    retries: '重试',
    latency: '延迟',
    tokens: 'Token',
    usage_source: '用量来源',
    fallback_reason: '回退原因',
  },

  // —— 用量来源枚举展示(值保持枚举原形)——
  usage_source: {
    upstream: '上游',
    parsed: '解析',
    estimated: '估算',
    missing: '缺失',
  },

  // —— 回退原因枚举展示(值保持原因码原形)——
  fallback_reason: {
    account_disabled: '主账号已禁用',
    account_cooling_down: '主账号冷却中（用量熔断 / 失败退避）',
    account_unhealthy: '主账号不健康（冷却过期未恢复）',
    account_unavailable: '主账号不可用',
    upstream_transport_error: '主账号上游连接失败',
    upstream_http: '主账号上游返回 {{code}}',
    other: '回退：{{reason}}',
  },

  // —— 筛选栏 ——
  filter: {
    aria: '用量筛选',
    preset_24h: '24 小时',
    preset_7d: '7 天',
    preset_30d: '30 天',
    from: '从',
    to: '到',
    status_success: '成功',
    status_failure: '失败',
    apply: '应用筛选',
  },

  // —— 时间粒度 ——
  granularity: {
    label: '时间粒度',
    auto: '自动',
    hour: '小时',
    day: '天',
  },

  // —— 概览 KPI ——
  stat: {
    logical_requests: '逻辑请求',
    upstream_attempts: '{{count}} 次上游尝试 · {{retries}} 次重试',
    success_rate: '成功率',
    failures: '{{count}} 次失败',
    total_tokens: 'Token 总量',
    final_accounting: '最终逻辑请求口径',
    avg_latency: '平均延迟',
    p95: 'P95 {{value}}',
  },

  // —— 趋势/构成/分布 ——
  metric: {
    total: 'Token 总量',
    input: '输入 Token',
    output: '输出 Token',
    reasoning: '推理 Token',
    cached: '缓存 Token',
    requests: '逻辑请求',
  },
  legend: {
    input: '输入',
    output: '输出',
    reasoning: '推理',
    cached: '缓存',
    total: '总计',
  },
  trend: {
    title: '用量趋势',
    subtitle: 'UTC 存储,按浏览器本地时区展示',
  },
  composition: {
    title: 'Token 构成',
    subtitle: '原始 Token 口径,不依赖价格配置',
  },
  distribution: {
    title: '模型 Token 分布',
    subtitle: '按逻辑模型与 Token 总量排序',
    empty: '暂无模型分布',
  },
  recent: {
    title: '最近活动',
    subtitle: '仅显示请求元数据,不包含 prompt / response 正文',
    empty: '暂无最近请求',
  },

  // —— 数值模板 ——
  value: {
    tokens: '{{count}} Token',
    requests_tokens: '{{count}} 请求 · {{tokens}} Token',
  },

  // —— 空态与图表通用 ——
  breakdown: {
    subtitle: '按 Token 总量排序',
    empty: '暂无分布数据',
  },
  latency: {
    title: '延迟诊断',
    subtitle: '按聚合维度显示平均延迟',
    dataset: '平均延迟(ms)',
    empty: '暂无延迟聚合',
  },

  // —— 空态 ——
  empty: {
    overview_title: '当前范围暂无用量',
    overview_desc: '调整时间范围或筛选条件后重试。Token 统计不依赖价格配置。',
    analysis_title: '当前范围暂无分析数据',
    analysis_desc: '分布数据由网关用量 API 提供。',
  },

  // —— 事件状态与行 ——
  event: {
    success: '成功',
    failure: '失败',
    retries_fallback: '{{count}} · 已回退',
    fallback_only: '已回退',
  },

  // —— 事件详情抽屉 ——
  detail: {
    aria: '请求事件详情',
    kicker: '请求事件',
    token_title: 'Token',
    token_subtitle: '最终逻辑请求口径,不因回退重复累计',
    attempts_title: '上游尝试',
    attempts_subtitle: '失败尝试没有可确认用量时不会虚构 Token',
    attempts_loading: '正在加载尝试明细…',
    attempts_empty_title: '没有独立尝试明细',
    attempts_empty_desc: '事件仍保留最终账号与重试次数。',
    primary_skipped: '主账号不可用，未向其发送请求；本次请求由回退账号完成。',
    error_summary: '脱敏错误摘要',
    no_body_notice: '安全边界:此详情不读取或显示 prompt、response body 或请求日志正文。',
  },

  // —— 事件表 ——
  events: {
    empty_title: '当前范围没有请求事件',
    empty_desc: '事件详情只包含元数据与脱敏错误摘要。',
    title: '请求事件',
    subtitle: '稳定游标分页 · 虚拟滚动',
  },

  // —— 页面级 ——
  page: {
    loading: '正在加载网关用量…',
  },

  // —— 页面级错误(本地化) ——
  error: {
    load_failed: '加载用量数据失败',
    load_more_failed: '加载更多事件失败',
    invalid_range: '开始时间必须早于结束时间',
  },
} as const;
