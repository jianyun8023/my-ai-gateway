// 网关用量三页文案(英文)——命名空间 console.usage
// 覆盖 Overview / Analysis / Request Events 三个标签页(单一 GatewayUsagePage)。
export const usage = {
  // —— 字段名(筛选、维度卡、事件表列、详情共用)——
  field: {
    request_id: 'Request ID',
    time: 'Time',
    model: 'Model',
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
    ttft: 'Time to first upstream data',
    tokens: 'Tokens',
    tps: 'TPS',
    cache: 'Cache',
    cache_hit_rate: 'Hit rate',
    usage_source: 'Usage source',
    fallback_reason: 'Fallback reason',
  },

  // —— 用量来源枚举展示(值保持枚举原形;筛选与详情用完整文案)——
  usage_source: {
    unknown: 'Unknown',
    upstream: 'Upstream (full response)',
    parsed: 'Upstream (stream)',
    estimated: 'Local estimate',
    missing: 'Not captured',
  },

  // —— 用量来源短文案(Badge 等紧凑场景)——
  usage_source_short: {
    unknown: 'Unknown',
    upstream: 'Upstream',
    parsed: 'Upstream stream',
    estimated: 'Estimated',
    missing: 'Not captured',
  },

  // —— 用量来源说明(Tooltip,区分数据可信来源与采集方式)——
  usage_source_desc: {
    unknown: 'Usage source is unknown; token provenance is unconfirmed.',
    upstream: 'Token usage was reported by the upstream in the usage field of the full response.',
    parsed: 'Token usage comes from the usage field of the upstream SSE stream, parsed and merged per event by the gateway. It is not a local estimate.',
    estimated: 'The upstream did not report token usage; values are estimated locally by the gateway.',
    missing: 'No token usage data is available.',
  },

  // —— 回退原因枚举展示(值保持原因码原形)——
  fallback_reason: {
    account_disabled: 'Primary account disabled',
    account_cooling_down: 'Primary account cooling down (usage cap / failure backoff)',
    account_unhealthy: 'Primary account unhealthy (cooldown expired, not recovered)',
    account_unavailable: 'Primary account unavailable',
    upstream_transport_error: 'Primary upstream connection error',
    upstream_http: 'Primary upstream returned {{code}}',
    other: 'Fallback: {{reason}}',
  },

  // —— 筛选栏 ——
  filter: {
    aria: 'Usage filters',
    preset_today: 'Today',
    preset_yesterday: 'Yesterday',
    preset_24h: 'Last 24 hours',
    preset_7d: 'Last 7 days',
    preset_30d: 'Last 30 days',
    preset_custom: 'Custom',
    from: 'From',
    to: 'To',
    status_success: 'Succeeded',
    status_failure: 'Failed',
    apply: 'Apply filters',
    advanced: 'Advanced Filters',
    reset: 'Reset',
    preset_active: 'Active',
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
    composition: 'Input / Output',
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
    cache_read: 'Cache Read',
    cache_creation: 'Cache Creation',
    total: 'Total',
  },
  trend: {
    empty: 'No trend data in this range',
    view_data: 'View data',
    hide_data: 'Hide data',
    composition_hint: 'Input and output stacked; reported total is available separately.',
    axes_hint: 'The two measures use separate axes.',
    title: 'Usage Trend',
    subtitle: 'Times shown in your local timezone',
  },
  composition: {
    unreported: 'Token usage is unavailable; an accounting zero is not a confirmed zero.',
    estimated: '{{count}} requests use estimated tokens, included in totals.',
    unknown: '{{count}} requests have an unknown usage source; token provenance is unconfirmed.',
    chart_title: 'Input / output split',
    empty: 'No input or output tokens reported',
    total_difference: 'Input + output: {{combined}}; reported total: {{total}}. The split uses input + output.',
    overlap: 'Reasoning and cache are separate reported measures, excluded from this split.',
    missing: 'Usage is missing for {{count}} requests and is not included in token totals.',
    title: 'Token Composition',
    cache_rate: 'Cache read / input: {{rate}}',
  },
  distribution: {
    requests: '{{count}} requests',
    top: 'Showing top {{count}} of {{total}} groups',
    title: 'Model Token Distribution',
    subtitle: 'Sorted by logical model · total tokens',
    empty: 'No model distribution yet',
  },
  recent: {
    title: 'Recent Activity',
    empty: 'No recent requests',
  },

  // —— 数值模板 ——
  value: {
    tokens: '{{count}} tokens',
    requests_tokens: '{{count}} requests · {{tokens}} tokens',
  },

  // —— 空态与图表通用 ——
  breakdown: {
    subtitle: 'Total tokens and share of the selected range',
    empty: 'No breakdown data yet',
  },
  latency: {
    title: 'Latency by Source',
    subtitle: 'Average latency, P95 and request counts by source',
    p95: 'P95 latency',
    dataset: 'Avg latency (ms)',
    empty: 'No latency aggregates yet',
  },

  // —— 空态 ——
  empty: {
    overview_title: 'No usage in the selected range',
    overview_desc: 'Try a different time range or filters.',
    analysis_title: 'No analysis data in the selected range',
    analysis_desc: 'Try a different time range or filters.',
  },

  // —— 事件状态与行 ——
  event: {
    retry_count_one: '{{count}} retry',
    retry_count_other: '{{count}} retries',
    success: 'Success',
    failure: 'Failure',
    retries_fallback: '{{count}} · fallback',
    fallback_only: 'fallback',
  },

  // —— 事件详情抽屉 ——
  detail: {
    aria: 'Request event details',
    kicker: 'Request Event',
    section_basic: 'Basic Info',
    section_routing: 'Routing',
    section_cache: 'Cache',
    token_title: 'Tokens',
    token_subtitle: 'Final logical request accounting; never duplicated across fallbacks',
    attempts_title: 'Upstream Attempts',
    attempts_subtitle: 'Failed attempts without reported usage add no tokens',
    attempts_loading: 'Loading attempt details…',
    attempts_empty_title: 'No standalone attempt details',
    attempts_empty_desc: 'The event retains the final account and retry count.',
    primary_skipped: 'The primary account was unavailable, so no request was sent to it; the fallback account served this event.',
    error_summary: 'Sanitized error summary',
  },

  // —— 事件表 ——
  events: {
    latency_basis: 'Total latency; bar length compares with the longest loaded request. Green ≤3s, amber 3–10s, red >10s; visual cues only.',
    view_aria: 'View request {{id}} details',
    empty_title: 'No request events in the selected range',
    empty_desc: 'Try a different time range or filters.',
    title: 'Request Events',
    token_details_title: 'Token usage details',
    tps_details_title: 'Average output speed',
    cache_details_title: 'Cache details',
    cache_basis: 'Hit rate = cache read / input tokens; cache creation is not counted as a hit',
    tps_basis: 'TPS = output tokens / total request latency in seconds: the average end-to-end output speed. Streaming and non-streaming requests use the same formula, without subtracting time to first upstream data',
  },

  // —— 页面级 ——
  page: {
    loading: 'Loading gateway usage…',
    refreshing: 'Refreshing gateway usage…',
  },

  // —— 页面级错误(本地化) ——
  error: {
    load_failed: 'Failed to load usage data',
    load_more_failed: 'Failed to load more events',
    invalid_range: 'Start time must be earlier than end time',
  },
} as const;
