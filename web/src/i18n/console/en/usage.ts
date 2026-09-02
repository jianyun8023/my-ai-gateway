// 网关用量三页文案(英文)——命名空间 console.usage
// 覆盖 Overview / Analysis / Request Events 三个标签页(单一 GatewayUsagePage)。
export const usage = {
  // —— 字段名(筛选、维度卡、事件表列、详情共用)——
  field: {
    time: 'Time',
    logical_model: 'Logical model',
    upstream_model: 'Upstream model',
    provider: 'Provider',
    source_id: 'Source ID',
    source_account: 'Source / Account',
    client_source: 'Client Source',
    account: 'Account',
    protocol: 'Protocol',
    protocol_in: 'Inbound protocol',
    protocol_upstream: 'Upstream protocol',
    virtual_key_id: 'Virtual Key ID',
    status: 'Status',
    retries: 'Retries',
    latency: 'Latency',
    tokens: 'Tokens',
    usage_source: 'Usage source',
  },

  // —— 用量来源枚举展示(值保持枚举原形)——
  usage_source: {
    upstream: 'Upstream',
    parsed: 'Parsed',
    estimated: 'Estimated',
    missing: 'Missing',
  },

  // —— 筛选栏 ——
  filter: {
    aria: 'Usage filters',
    preset_24h: 'Last 24 hours',
    preset_7d: 'Last 7 days',
    preset_30d: 'Last 30 days',
    from: 'From',
    to: 'To',
    status_success: 'Succeeded',
    status_failure: 'Failed',
    apply: 'Apply filters',
  },

  // —— 时间粒度 ——
  granularity: {
    label: 'Time granularity',
    auto: 'Auto',
    hour: 'Hourly',
    day: 'Daily',
  },

  // —— 概览 KPI ——
  stat: {
    logical_requests: 'Logical Requests',
    upstream_attempts: '{{count}} upstream attempts · {{retries}} retries',
    success_rate: 'Success Rate',
    failures: '{{count}} failures',
    total_tokens: 'Total Tokens',
    final_accounting: 'Final logical request accounting',
    avg_latency: 'Average Latency',
    p95: 'P95 {{value}}',
  },

  // —— 趋势/构成/分布 ——
  metric: {
    total: 'Total Tokens',
    input: 'Input Tokens',
    output: 'Output Tokens',
    reasoning: 'Reasoning Tokens',
    cached: 'Cached Tokens',
    requests: 'Logical Requests',
  },
  legend: {
    input: 'Input',
    output: 'Output',
    reasoning: 'Reasoning',
    cached: 'Cached',
    total: 'Total',
  },
  trend: {
    title: 'Usage Trend',
    subtitle: 'Stored in UTC, displayed in your local time zone',
  },
  composition: {
    title: 'Token Composition',
    subtitle: 'Raw token counts, independent of pricing configuration',
  },
  distribution: {
    title: 'Model Token Distribution',
    subtitle: 'Sorted by logical model · total tokens',
    empty: 'No model distribution yet',
  },
  recent: {
    title: 'Recent Activity',
    subtitle: 'Request metadata only; prompt / response bodies are not stored',
    empty: 'No recent requests',
  },

  // —— 数值模板 ——
  value: {
    tokens: '{{count}} tokens',
    requests_tokens: '{{count}} requests · {{tokens}} tokens',
  },

  // —— 空态与图表通用 ——
  breakdown: {
    subtitle: 'Sorted by total tokens',
    empty: 'No breakdown data yet',
  },
  latency: {
    title: 'Latency Diagnostics',
    subtitle: 'Average latency by aggregate dimension',
    dataset: 'Avg latency (ms)',
    empty: 'No latency aggregates yet',
  },

  // —— 空态 ——
  empty: {
    overview_title: 'No usage in the selected range',
    overview_desc: 'Adjust the time range or filters and try again. Token statistics do not depend on pricing configuration.',
    analysis_title: 'No analysis data in the selected range',
    analysis_desc: 'Breakdown data is provided by the gateway usage API.',
  },

  // —— 事件状态与行 ——
  event: {
    success: 'Success',
    failure: 'Failure',
    retries_fallback: '{{count}} · fallback',
  },

  // —— 事件详情抽屉 ——
  detail: {
    aria: 'Request event details',
    kicker: 'Request Event',
    token_title: 'Tokens',
    token_subtitle: 'Final logical request accounting; never duplicated across fallbacks',
    attempts_title: 'Upstream Attempts',
    attempts_subtitle: 'Failed attempts without confirmed usage do not fabricate tokens',
    attempts_loading: 'Loading attempt details…',
    attempts_empty_title: 'No standalone attempt details',
    attempts_empty_desc: 'The event retains the final account and retry count.',
    error_summary: 'Sanitized error summary',
    no_body_notice: 'Privacy boundary: this view never reads or displays prompt, response body, or request log bodies.',
  },

  // —— 事件表 ——
  events: {
    empty_title: 'No request events in the selected range',
    empty_desc: 'Event details contain metadata and sanitized error summaries only.',
    title: 'Request Events',
    subtitle: 'Stable cursor pagination · virtual scrolling',
  },

  // —— 页面级 ——
  page: {
    loading: 'Loading gateway usage…',
  },

  // —— 页面级错误(本地化) ——
  error: {
    load_failed: 'Failed to load usage data',
    load_more_failed: 'Failed to load more events',
    invalid_range: 'Start time must be earlier than end time',
  },
} as const;
