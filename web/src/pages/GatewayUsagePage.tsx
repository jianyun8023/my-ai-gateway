import { useCallback, useEffect, useRef, useState, type CSSProperties } from 'react';
import { useVirtualizer } from '@tanstack/react-virtual';
import { Bar, Doughnut, Line } from 'react-chartjs-2';
import '@/lib/chartjs';
import { Button } from '@/components/ui/Button';
import { Card } from '@/components/ui/Card';
import { EmptyState } from '@/components/ui/EmptyState';
import { GatewayUsageClient, type GatewayUsageFilters, type UsageBreakdownDimension, type UsageBreakdownItem, type UsageEventViewModel, type UsageOverviewViewModel, type UsageSummaryViewModel } from '@/gateway-usage';
import type { GatewayUsageTab } from '@/lib/consoleNavigation';
import styles from './GatewayUsagePage.module.scss';

const FILTER_STORAGE_KEY = 'my-ai-gateway-usage-filters-v2';
const COLUMNS_STORAGE_KEY = 'my-ai-gateway-usage-event-columns-v2';

const EVENT_COLUMNS = [
  'time',
  'logicalModel',
  'upstreamModel',
  'provider',
  'sourceAccount',
  'clientSource',
  'protocol',
  'status',
  'retries',
  'latency',
  'tokens',
  'usageSource',
] as const;

type EventColumn = typeof EVENT_COLUMNS[number];

const EVENT_COLUMN_LABELS: Record<EventColumn, string> = {
  time: '时间',
  logicalModel: 'Logical model',
  upstreamModel: 'Upstream model',
  provider: 'Provider',
  sourceAccount: 'Source / Account',
  clientSource: 'Client Source',
  protocol: '协议',
  status: '状态',
  retries: '重试',
  latency: '延迟',
  tokens: 'Token',
  usageSource: 'Usage source',
};

const DEFAULT_VISIBLE_COLUMNS: EventColumn[] = [
  'time',
  'logicalModel',
  'upstreamModel',
  'provider',
  'sourceAccount',
  'clientSource',
  'protocol',
  'status',
  'retries',
  'latency',
  'tokens',
  'usageSource',
];

const DAY_MS = 24 * 60 * 60 * 1000;

const defaultFilters = (): GatewayUsageFilters => {
  const to = new Date();
  const from = new Date(to.getTime() - DAY_MS);
  return { from: from.toISOString(), to: to.toISOString() };
};

const safeLocalRead = (key: string): string => {
  try {
    return localStorage.getItem(key) ?? '';
  } catch {
    return '';
  }
};

const safeParseFilters = (): GatewayUsageFilters => {
  const fallback = defaultFilters();
  try {
    const value = JSON.parse(safeLocalRead(FILTER_STORAGE_KEY)) as Partial<GatewayUsageFilters>;
    if (!value.from || !value.to || Number.isNaN(Date.parse(value.from)) || Number.isNaN(Date.parse(value.to))) {
      return fallback;
    }
    return { ...fallback, ...value };
  } catch {
    return fallback;
  }
};

export const normalizeVisibleEventColumns = (value: unknown): EventColumn[] => {
  if (!Array.isArray(value)) return DEFAULT_VISIBLE_COLUMNS;
  const normalized = EVENT_COLUMNS.filter((column) => value.includes(column));
  return normalized.length > 0 ? normalized : DEFAULT_VISIBLE_COLUMNS;
};

const loadVisibleColumns = (): EventColumn[] => {
  try {
    return normalizeVisibleEventColumns(JSON.parse(safeLocalRead(COLUMNS_STORAGE_KEY)));
  } catch {
    return DEFAULT_VISIBLE_COLUMNS;
  }
};

export const appendStableEventPage = (
  current: readonly UsageEventViewModel[],
  incoming: readonly UsageEventViewModel[],
): UsageEventViewModel[] => {
  const seen = new Set(current.map((event) => `${event.id}:${event.createdAt}`));
  return [...current, ...incoming.filter((event) => {
    const key = `${event.id}:${event.createdAt}`;
    if (seen.has(key)) return false;
    seen.add(key);
    return true;
  })];
};

const toLocalInputValue = (iso: string): string => {
  const date = new Date(iso);
  const offsetMs = date.getTimezoneOffset() * 60 * 1000;
  return new Date(date.getTime() - offsetMs).toISOString().slice(0, 16);
};

const fromLocalInputValue = (value: string): string => new Date(value).toISOString();

const formatNumber = (value: number): string => new Intl.NumberFormat(undefined, {
  notation: value >= 100_000 ? 'compact' : 'standard',
  maximumFractionDigits: 1,
}).format(value);

const formatTime = (value: string): string => {
  if (!value) return '—';
  return new Intl.DateTimeFormat(undefined, {
    dateStyle: 'short',
    timeStyle: 'medium',
  }).format(new Date(value));
};

const formatBucket = (value: string): string => new Intl.DateTimeFormat(undefined, {
  month: 'short',
  day: 'numeric',
  hour: '2-digit',
  minute: '2-digit',
}).format(new Date(value));

const formatDuration = (value: number): string => {
  if (value <= 0) return '—';
  if (value < 1000) return `${formatNumber(value)} ms`;
  return `${(value / 1000).toFixed(value < 10_000 ? 1 : 0)} s`;
};

const makeChartColors = () => [
  'oklch(58% 0.16 145)',
  'oklch(74% 0.08 195)',
  'oklch(70% 0.16 80)',
  'oklch(78% 0.05 220)',
  'oklch(58% 0.2 25)',
  'oklch(66% 0.08 240)',
  'oklch(68% 0.1 125)',
];

interface FilterBarProps {
  draft: GatewayUsageFilters;
  onChange: (value: GatewayUsageFilters) => void;
  onApply: () => void;
  loading: boolean;
}

function FilterBar({ draft, onChange, onApply, loading }: FilterBarProps) {
  const update = (field: keyof GatewayUsageFilters, value: string) => onChange({
    ...draft,
    [field]: value || undefined,
  });
  const setPreset = (durationMs: number) => {
    const to = new Date();
    onChange({ ...draft, from: new Date(to.getTime() - durationMs).toISOString(), to: to.toISOString() });
  };

  return (
    <section className={styles.filters} aria-label="用量筛选">
      <div className={styles.filterPresets}>
        <Button size="sm" variant="ghost" onClick={() => setPreset(DAY_MS)}>24 小时</Button>
        <Button size="sm" variant="ghost" onClick={() => setPreset(7 * DAY_MS)}>7 天</Button>
        <Button size="sm" variant="ghost" onClick={() => setPreset(30 * DAY_MS)}>30 天</Button>
      </div>
      <label>从<input type="datetime-local" value={toLocalInputValue(draft.from)} onChange={(event) => update('from', fromLocalInputValue(event.target.value))} /></label>
      <label>到<input type="datetime-local" value={toLocalInputValue(draft.to)} onChange={(event) => update('to', fromLocalInputValue(event.target.value))} /></label>
      <label>Logical model<input value={draft.logicalModel ?? ''} onChange={(event) => update('logicalModel', event.target.value)} placeholder="全部" /></label>
      <label>Upstream model<input value={draft.upstreamModel ?? ''} onChange={(event) => update('upstreamModel', event.target.value)} placeholder="全部" /></label>
      <label>Provider<input value={draft.provider ?? ''} onChange={(event) => update('provider', event.target.value)} placeholder="全部" /></label>
      <label>Source ID<input value={draft.sourceId ?? ''} onChange={(event) => update('sourceId', event.target.value)} placeholder="全部" /></label>
      <label>Client Source<input value={draft.clientSource ?? ''} onChange={(event) => update('clientSource', event.target.value)} placeholder="全部" /></label>
      <label>Account<input value={draft.account ?? ''} onChange={(event) => update('account', event.target.value)} placeholder="全部" /></label>
      <label>入站协议<input value={draft.protocolIn ?? ''} onChange={(event) => update('protocolIn', event.target.value)} placeholder="全部" /></label>
      <label>上游协议<input value={draft.protocolUpstream ?? ''} onChange={(event) => update('protocolUpstream', event.target.value)} placeholder="全部" /></label>
      <label>Virtual Key ID<input inputMode="numeric" value={draft.virtualKey ?? ''} onChange={(event) => update('virtualKey', event.target.value)} placeholder="全部" /></label>
      <label>状态<select value={draft.status ?? ''} onChange={(event) => update('status', event.target.value)}><option value="">全部</option><option value="success">成功</option><option value="failure">失败</option></select></label>
      <label>Usage source<select value={draft.usageSource ?? ''} onChange={(event) => update('usageSource', event.target.value)}><option value="">全部</option><option value="upstream">upstream</option><option value="parsed">parsed</option><option value="estimated">estimated</option><option value="missing">missing</option></select></label>
      <Button className={styles.applyFilters} variant="secondary" onClick={onApply} loading={loading}>应用筛选</Button>
    </section>
  );
}

function Stat({ label, value, hint, tone }: { label: string; value: string; hint?: string; tone?: 'success' | 'warning' }) {
  return (
    <div className={styles.stat} data-tone={tone}>
      <span>{label}</span>
      <strong>{value}</strong>
      {hint && <small>{hint}</small>}
    </div>
  );
}

type TrendMetric = 'total' | 'input' | 'output' | 'reasoning' | 'cached' | 'requests';

const TREND_METRICS: Array<{ value: TrendMetric; label: string }> = [
  { value: 'total', label: 'Total Token' },
  { value: 'input', label: 'Input Token' },
  { value: 'output', label: 'Output Token' },
  { value: 'reasoning', label: 'Reasoning Token' },
  { value: 'cached', label: 'Cached Token' },
  { value: 'requests', label: '逻辑请求' },
];

function extractMetricData(data: UsageOverviewViewModel, metric: TrendMetric): number[] {
  switch (metric) {
    case 'total': return data.timeseries.map((p) => p.tokens.total);
    case 'input': return data.timeseries.map((p) => p.tokens.input);
    case 'output': return data.timeseries.map((p) => p.tokens.output);
    case 'reasoning': return data.timeseries.map((p) => p.tokens.reasoning);
    case 'cached': return data.timeseries.map((p) => p.tokens.cached);
    case 'requests': return data.timeseries.map((p) => p.logicalRequests);
  }
}

function TokenComposition({ summary }: { summary: UsageSummaryViewModel }) {
  const colors = makeChartColors();
  const rows = [
    { label: 'Input', value: summary.tokens.input, color: colors[0] },
    { label: 'Output', value: summary.tokens.output, color: colors[1] },
    { label: 'Reasoning', value: summary.tokens.reasoning, color: colors[2] },
    { label: 'Cached', value: summary.tokens.cached, color: colors[3] },
  ];
  const compositionData = {
    labels: rows.map((row) => row.label),
    datasets: [{ data: rows.map((row) => row.value), backgroundColor: rows.map((row) => row.color), borderWidth: 0 }],
  };

  return (
    <Card title="Token 构成" subtitle="原始 Token 口径，不依赖价格配置">
      <div className={styles.tokenComposition}>
        <div className={styles.chartSmall}><Doughnut data={compositionData} options={{ responsive: true, maintainAspectRatio: false, cutout: '70%', plugins: { legend: { display: false } } }} /></div>
        <dl>{rows.map((row) => <div key={row.label}><dt><i style={{ background: row.color }} />{row.label}</dt><dd>{formatNumber(row.value)}</dd></div>)}</dl>
      </div>
    </Card>
  );
}

function ModelDistribution({ rows }: { rows: UsageBreakdownItem[] }) {
  const visibleRows = rows.slice(0, 6);
  const maxTokens = Math.max(...visibleRows.map((row) => row.tokens.total), 1);

  return (
    <Card title="模型 Token 分布" subtitle="按 Logical model · Total Token 排序">
      {visibleRows.length === 0 ? <EmptyState title="暂无模型分布" /> : (
        <div className={styles.distributionList}>{visibleRows.map((row) => (
          <div key={row.key}>
            <div><strong>{row.label}</strong><span>{formatNumber(row.logicalRequests)} 请求 · {formatNumber(row.tokens.total)} Token</span></div>
            <span className={styles.distributionTrack}><i style={{ width: `${Math.max(4, (row.tokens.total / maxTokens) * 100)}%` }} /></span>
          </div>
        ))}</div>
      )}
    </Card>
  );
}

function Overview({ data, metric, onMetricChange }: { data: UsageOverviewViewModel; metric: TrendMetric; onMetricChange: (m: TrendMetric) => void }) {
  const { summary } = data;
  const hasData = summary.logicalRequests > 0 || summary.tokens.total > 0;
  if (!hasData) {
    return <EmptyState title="当前范围暂无用量" description="调整时间范围或筛选条件后重试。Token 统计不依赖价格配置。" />;
  }
  const colors = makeChartColors();
  const metricLabel = TREND_METRICS.find((m) => m.value === metric)?.label ?? 'Total Token';
  const secondaryMetric = metric === 'requests' ? 'total' : 'requests';
  const secondaryLabel = secondaryMetric === 'requests' ? '逻辑请求' : 'Total Token';
  const trendData = {
    labels: data.timeseries.map((point) => formatBucket(point.bucket)),
    datasets: [
      { label: metricLabel, data: extractMetricData(data, metric), borderColor: colors[0], backgroundColor: 'oklch(58% 0.16 145 / 0.14)', fill: true, tension: 0.35 },
      { label: secondaryLabel, data: extractMetricData(data, secondaryMetric), borderColor: colors[1], backgroundColor: colors[1], tension: 0.35, yAxisID: 'secondary' },
    ],
  };

  return (
    <div className={styles.stack}>
      <div className={styles.statsGrid} data-od-id="kpi-row">
        <Stat label="逻辑请求" value={formatNumber(summary.logicalRequests)} hint={`${formatNumber(summary.upstreamAttempts)} 次上游尝试 · ${formatNumber(summary.retries)} 次重试`} />
        <Stat label="成功率" value={`${(summary.successRate * 100).toFixed(1)}%`} hint={`${summary.failedRequests} 次失败`} tone={summary.failedRequests > 0 ? 'warning' : 'success'} />
        <Stat label="Total Token" value={formatNumber(summary.tokens.total)} hint="最终逻辑请求口径" />
        <Stat label="平均延迟" value={formatDuration(summary.averageLatencyMs)} hint={`P95 ${formatDuration(summary.p95LatencyMs)}`} />
      </div>
      <div className={styles.chartGrid}>
        <Card title="用量趋势" subtitle="UTC 存储，按浏览器本地时区展示" data-od-id="token-trend" extra={
          <select value={metric} onChange={(e) => onMetricChange(e.target.value as TrendMetric)} className={styles.metricSelect}>
            {TREND_METRICS.map((m) => <option key={m.value} value={m.value}>{m.label}</option>)}
          </select>
        }>
          <div className={styles.chartLarge}><Line data={trendData} options={{ responsive: true, maintainAspectRatio: false, interaction: { mode: 'index', intersect: false }, scales: { secondary: { position: 'right', grid: { display: false } } } }} /></div>
        </Card>
        <TokenComposition summary={summary} />
      </div>
      <div className={styles.overviewLowerGrid}>
        <Card title="最近活动" subtitle="仅显示请求元数据，不包含 prompt / response 正文" data-od-id="recent-activity">
          {data.recentEvents.length === 0 ? <EmptyState title="暂无最近请求" /> : (
            <div className={styles.recentList}>{data.recentEvents.map((event) => (
              <div key={`${event.id}:${event.createdAt}`}>
                <span className={styles.statusDot} data-success={event.success} />
                <time>{formatTime(event.createdAt)}</time>
                <strong>{event.logicalModel}</strong>
                <span>{event.provider} · {event.sourceId} / {event.account}</span>
                <em>{formatNumber(event.tokens.total)} Token</em>
              </div>
            ))}</div>
          )}
        </Card>
        <div data-od-id="model-distribution"><ModelDistribution rows={data.logicalModels} /></div>
      </div>
    </div>
  );
}

const ANALYSIS_DIMENSIONS: Array<{ dimension: UsageBreakdownDimension; title: string }> = [
  { dimension: 'logical_model', title: 'Logical model' },
  { dimension: 'upstream_model', title: 'Upstream model' },
  { dimension: 'provider', title: 'Provider' },
  { dimension: 'source_id', title: 'Source' },
  { dimension: 'client_source', title: 'Client Source' },
  { dimension: 'account', title: 'Account' },
  { dimension: 'protocol_in', title: '入站协议' },
  { dimension: 'protocol_upstream', title: '上游协议' },
];

function BreakdownChart({ title, rows }: { title: string; rows: UsageBreakdownItem[] }) {
  const visibleRows = rows.slice(0, 8);
  return (
    <Card title={title} subtitle="按 Total Token 排序">
      {visibleRows.length === 0 ? <EmptyState title="暂无分布数据" /> : (
        <div className={styles.breakdownChart}>
          <Bar data={{ labels: visibleRows.map((item) => item.label), datasets: [{ label: 'Total Token', data: visibleRows.map((item) => item.tokens.total), backgroundColor: makeChartColors()[0], borderRadius: 6 }] }} options={{ indexAxis: 'y', responsive: true, maintainAspectRatio: false, plugins: { legend: { display: false } } }} />
        </div>
      )}
    </Card>
  );
}

function Analysis({ breakdowns, summary }: { breakdowns: Partial<Record<UsageBreakdownDimension, UsageBreakdownItem[]>>; summary: UsageSummaryViewModel }) {
  const allRows = Object.values(breakdowns).flatMap((rows) => rows ?? []);
  const latencyRows = [...allRows].filter((row) => row.averageLatencyMs !== undefined).sort((a, b) => (b.averageLatencyMs ?? 0) - (a.averageLatencyMs ?? 0)).slice(0, 8);
  if (allRows.length === 0 && summary.tokens.total === 0) return <EmptyState title="当前范围暂无分析数据" description="分布数据由网关 Usage breakdown API 提供。" />;
  return (
    <div className={styles.stack}>
      <TokenComposition summary={summary} />
      <div className={styles.analysisGrid}>
        {ANALYSIS_DIMENSIONS.map(({ dimension, title }) => <BreakdownChart key={dimension} title={title} rows={breakdowns[dimension] ?? []} />)}
        <Card title="延迟诊断" subtitle="按聚合维度显示平均延迟">
          {latencyRows.length === 0 ? <EmptyState title="暂无延迟聚合" /> : (
            <div className={styles.breakdownChart}>
              <Bar data={{ labels: latencyRows.map((item) => item.label), datasets: [{ label: '平均延迟（ms）', data: latencyRows.map((item) => item.averageLatencyMs ?? 0), backgroundColor: makeChartColors()[2], borderRadius: 6 }] }} options={{ responsive: true, maintainAspectRatio: false, plugins: { legend: { display: false } } }} />
            </div>
          )}
        </Card>
      </div>
    </div>
  );
}

function UsageBadge({ source }: { source: string }) {
  return <span className={styles.usageBadge} data-source={source}>{source}</span>;
}

const renderEventCell = (event: UsageEventViewModel, column: EventColumn) => {
  switch (column) {
    case 'time': return <time>{formatTime(event.createdAt)}</time>;
    case 'logicalModel': return <strong>{event.logicalModel}</strong>;
    case 'upstreamModel': return event.upstreamModel;
    case 'provider': return event.provider;
    case 'sourceAccount': return <span>{event.sourceId}<small>{event.account}</small></span>;
    case 'clientSource': return event.clientSource;
    case 'protocol': return <span>{event.protocolIn}<small>→ {event.protocolUpstream}</small></span>;
    case 'status': return <span className={styles.statusBadge} data-success={event.success}>{event.statusCode || '—'} · {event.success ? '成功' : '失败'}</span>;
    case 'retries': return event.fallback ? `${event.retryCount} · fallback` : String(event.retryCount);
    case 'latency': return `${formatNumber(event.latencyMs)} ms`;
    case 'tokens': return formatNumber(event.tokens.total);
    case 'usageSource': return <UsageBadge source={event.usageSource} />;
  }
};

interface UsageAttemptDetail {
  attemptNo: number;
  provider: string;
  sourceId: string;
  account: string;
  upstreamModel: string;
  statusCode: number;
  success: boolean;
  latencyMs: number;
}

function EventDetails({ event, onClose, client }: { event: UsageEventViewModel; onClose: () => void; client: GatewayUsageClient }) {
  const [attempts, setAttempts] = useState<UsageAttemptDetail[]>([]);
  const [loadingAttempts, setLoadingAttempts] = useState(true);
  const [loadedRequestId, setLoadedRequestId] = useState(event.requestId);
  if (loadedRequestId !== event.requestId) {
    setLoadedRequestId(event.requestId);
    setAttempts([]);
    setLoadingAttempts(true);
  }

  useEffect(() => {
    let cancelled = false;
    client.eventDetail(event.requestId).then((data) => {
      if (cancelled) return;
      const detail = data as { attempts?: Array<{ attempt_no?: number; provider_id?: string; source_id?: string; account_id?: string; upstream_model_id?: string; status_code?: number; success?: boolean; latency_ms?: number }> };
      setAttempts((detail.attempts ?? []).map((a) => ({
        attemptNo: a.attempt_no ?? 0,
        provider: a.provider_id ?? '—',
        sourceId: a.source_id ?? 'unknown',
        account: a.account_id ?? '—',
        upstreamModel: a.upstream_model_id ?? '—',
        statusCode: a.status_code ?? 0,
        success: a.success ?? false,
        latencyMs: a.latency_ms ?? 0,
      })));
    }).catch(() => {
      if (!cancelled) setAttempts([]);
    }).finally(() => {
      if (!cancelled) setLoadingAttempts(false);
    });
    return () => { cancelled = true; };
  }, [client, event.requestId]);

  const displayAttempts = attempts.length > 0 ? attempts : event.attempts.map((a) => ({
    attemptNo: a.attemptIndex,
    provider: a.provider,
    sourceId: a.sourceId,
    account: a.account,
    upstreamModel: a.upstreamModel,
    statusCode: a.statusCode,
    success: a.success,
    latencyMs: a.latencyMs,
  }));

  return (
    <div className={styles.drawerBackdrop} role="presentation" onMouseDown={(mouseEvent) => mouseEvent.target === mouseEvent.currentTarget && onClose()}>
      <aside className={styles.drawer} role="dialog" aria-modal="true" aria-label="请求事件详情" data-od-id="event-drawer">
        <header><div><span>Request Event</span><h2>{event.requestId}</h2></div><Button variant="ghost" onClick={onClose}>关闭</Button></header>
        <section className={styles.detailGrid}>
          <div><span>时间</span><strong>{formatTime(event.createdAt)}</strong></div>
          <div><span>状态</span><strong>{event.statusCode} · {event.success ? '成功' : '失败'}</strong></div>
          <div><span>Logical model</span><strong>{event.logicalModel}</strong></div>
          <div><span>Upstream model</span><strong>{event.upstreamModel}</strong></div>
          <div><span>Provider</span><strong>{event.provider}</strong></div>
          <div><span>Source</span><strong>{event.sourceId}</strong></div>
          <div><span>Client Source</span><strong>{event.clientSource}</strong></div>
          <div><span>Account</span><strong>{event.account}</strong></div>
          <div><span>协议</span><strong>{event.protocolIn} → {event.protocolUpstream}</strong></div>
          <div><span>Usage source</span><strong><UsageBadge source={event.usageSource} /></strong></div>
          <div><span>延迟</span><strong>{formatNumber(event.latencyMs)} ms</strong></div>
          <div><span>重试</span><strong>{event.retryCount}{event.fallback ? ' · fallback' : ''}</strong></div>
        </section>
        <Card title="Token" subtitle="最终逻辑请求口径，不因 fallback 重复累计">
          <div className={styles.tokenDetails}><span>Input <strong>{event.tokens.input}</strong></span><span>Output <strong>{event.tokens.output}</strong></span><span>Reasoning <strong>{event.tokens.reasoning}</strong></span><span>Cached <strong>{event.tokens.cached}</strong></span><span>Total <strong>{event.tokens.total}</strong></span></div>
        </Card>
        <Card title="上游尝试" subtitle="失败尝试没有可确认 usage 时不会虚构 Token">
          {loadingAttempts ? <div style={{ padding: '1rem', opacity: 0.6 }}>加载 attempt 明细…</div> : displayAttempts.length === 0 ? <EmptyState title="没有独立 attempt 明细" description="事件仍保留最终账号与 retry_count。" /> : (
            <ol className={styles.attemptList}>{displayAttempts.map((attempt) => <li key={attempt.attemptNo}><span>#{attempt.attemptNo + 1}</span><strong>{attempt.account}</strong><span>{attempt.sourceId} · {attempt.provider} · {attempt.upstreamModel}</span><span className={attempt.success ? styles.statusSuccess : styles.statusFailure}>{attempt.statusCode} · {attempt.latencyMs} ms</span></li>)}</ol>
          )}
        </Card>
        {event.errorSummary && <Card title="脱敏错误摘要"><p className={styles.errorSummary}>{event.errorSummary}</p></Card>}
        <p className={styles.noBodyNotice}>安全边界：此详情不读取或显示 prompt、response body 或请求日志正文。</p>
      </aside>
    </div>
  );
}

interface EventsTableProps {
  events: UsageEventViewModel[];
  hasMore: boolean;
  loadingMore: boolean;
  onLoadMore: () => void;
  visibleColumns: EventColumn[];
  onVisibleColumnsChange: (columns: EventColumn[]) => void;
  onExport: (format: 'csv' | 'json') => void;
  client: GatewayUsageClient;
}

function EventsTable({ events, hasMore, loadingMore, onLoadMore, visibleColumns, onVisibleColumnsChange, onExport, client }: EventsTableProps) {
  const parentRef = useRef<HTMLDivElement>(null);
  const [selectedEvent, setSelectedEvent] = useState<UsageEventViewModel>();
  // TanStack Virtual intentionally exposes imperative measurement helpers.
  // eslint-disable-next-line react-hooks/incompatible-library
  const virtualizer = useVirtualizer({
    count: events.length,
    getScrollElement: () => parentRef.current,
    estimateSize: () => 58,
    overscan: 10,
  });
  const virtualItems = virtualizer.getVirtualItems();
  const lastIndex = virtualItems.at(-1)?.index ?? -1;
  useEffect(() => {
    if (hasMore && !loadingMore && lastIndex >= events.length - 5) onLoadMore();
  }, [events.length, hasMore, lastIndex, loadingMore, onLoadMore]);

  if (events.length === 0) return <EmptyState title="当前范围没有请求事件" description="事件详情只包含元数据与脱敏错误摘要。" />;

  return (
    <Card variant="flush" title="请求事件" subtitle="稳定游标分页 · 虚拟滚动" data-od-id="events-table" extra={<div className={styles.eventActions}><details><summary>列偏好</summary><div className={styles.columnMenu}>{EVENT_COLUMNS.map((column) => <label key={column}><input type="checkbox" checked={visibleColumns.includes(column)} onChange={() => onVisibleColumnsChange(visibleColumns.includes(column) ? visibleColumns.filter((item) => item !== column) : EVENT_COLUMNS.filter((item) => visibleColumns.includes(item) || item === column))} />{EVENT_COLUMN_LABELS[column]}</label>)}</div></details><Button size="sm" variant="secondary" onClick={() => onExport('csv')}>导出 CSV</Button><Button size="sm" variant="secondary" onClick={() => onExport('json')}>导出 JSON</Button></div>}>
      <div className={styles.eventTable} style={{ '--event-columns': visibleColumns.length } as CSSProperties}>
        <div className={styles.eventHeader}>{visibleColumns.map((column) => <span key={column}>{EVENT_COLUMN_LABELS[column]}</span>)}</div>
        <div ref={parentRef} className={styles.eventScroll}>
          <div style={{ height: virtualizer.getTotalSize(), position: 'relative' }}>
            {virtualItems.map((virtualRow) => {
              const event = events[virtualRow.index];
              return (
                <button key={`${event.id}:${event.createdAt}`} className={styles.eventRow} style={{ transform: `translateY(${virtualRow.start}px)` }} onClick={() => setSelectedEvent(event)}>
                  {visibleColumns.map((column) => <span key={column}>{renderEventCell(event, column)}</span>)}
                </button>
              );
            })}
          </div>
          {loadingMore && <div className={styles.loadingMore}>加载更多事件…</div>}
        </div>
      </div>
      {selectedEvent && <EventDetails event={selectedEvent} onClose={() => setSelectedEvent(undefined)} client={client} />}
    </Card>
  );
}

interface GatewayUsagePageProps {
  activeTab: GatewayUsageTab;
  getAdminKey: () => string;
  refreshRevision: number;
  onLoadingChange?: (loading: boolean) => void;
}

export function GatewayUsagePage({
  activeTab,
  getAdminKey,
  refreshRevision,
  onLoadingChange,
}: GatewayUsagePageProps) {
  const getAdminKeyRef = useRef(getAdminKey);
  getAdminKeyRef.current = getAdminKey;
  const [client] = useState(() => new GatewayUsageClient({ getAdminKey: () => getAdminKeyRef.current() }));
  const [draftFilters, setDraftFilters] = useState<GatewayUsageFilters>(safeParseFilters);
  const [filters, setFilters] = useState<GatewayUsageFilters>(draftFilters);
  const [overview, setOverview] = useState<UsageOverviewViewModel>();
  const [analysisSummary, setAnalysisSummary] = useState<UsageSummaryViewModel>();
  const [breakdowns, setBreakdowns] = useState<Partial<Record<UsageBreakdownDimension, UsageBreakdownItem[]>>>({});
  const [events, setEvents] = useState<UsageEventViewModel[]>([]);
  const [nextCursor, setNextCursor] = useState<string>();
  const [hasMore, setHasMore] = useState(false);
  const [loading, setLoading] = useState(true);
  const [loadingMore, setLoadingMore] = useState(false);
  const [error, setError] = useState('');
  const [visibleColumns, setVisibleColumns] = useState<EventColumn[]>(loadVisibleColumns);
  const [trendMetric, setTrendMetric] = useState<TrendMetric>('total');
  const [granularity, setGranularity] = useState<'auto' | 'hour' | 'day'>('auto');

  const loadActiveTab = useCallback(async (signal?: AbortSignal) => {
    setLoading(true);
    onLoadingChange?.(true);
    setError('');
    try {
      if (activeTab === 'overview') {
        const granularityParam = granularity === 'auto' ? undefined : granularity;
        setOverview(await client.overview(filters, granularityParam, signal));
      } else if (activeTab === 'analysis') {
        const [summary, results] = await Promise.all([
          client.summary(filters, signal),
          Promise.all(ANALYSIS_DIMENSIONS.map(async ({ dimension }) => [
            dimension,
            await client.breakdown(filters, dimension, signal),
          ] as const)),
        ]);
        setAnalysisSummary(summary);
        setBreakdowns(Object.fromEntries(results));
      } else {
        const page = await client.events({ filters, limit: 100 }, signal);
        setEvents(page.events);
        setNextCursor(page.nextCursor);
        setHasMore(page.hasMore);
      }
    } catch (loadError) {
      if (signal?.aborted) return;
      setError(loadError instanceof Error ? loadError.message : '加载用量数据失败');
    } finally {
      if (!signal?.aborted) {
        setLoading(false);
        onLoadingChange?.(false);
      }
    }
  }, [activeTab, client, filters, granularity, onLoadingChange]);

  useEffect(() => {
    const controller = new AbortController();
    void loadActiveTab(controller.signal);
    return () => controller.abort();
  }, [loadActiveTab, refreshRevision]);

  useEffect(() => () => onLoadingChange?.(false), [onLoadingChange]);

  const applyFilters = () => {
    if (new Date(draftFilters.from) >= new Date(draftFilters.to)) {
      setError('开始时间必须早于结束时间');
      return;
    }
    localStorage.setItem(FILTER_STORAGE_KEY, JSON.stringify(draftFilters));
    setFilters({ ...draftFilters });
  };

  const loadMore = useCallback(async () => {
    if (!nextCursor || !hasMore || loadingMore) return;
    setLoadingMore(true);
    try {
      const page = await client.events({ filters, cursor: nextCursor, limit: 100 });
      setEvents((current) => appendStableEventPage(current, page.events));
      setNextCursor(page.nextCursor);
      setHasMore(page.hasMore);
    } catch (loadError) {
      setError(loadError instanceof Error ? loadError.message : '加载更多事件失败');
    } finally {
      setLoadingMore(false);
    }
  }, [client, filters, hasMore, loadingMore, nextCursor]);

  const changeVisibleColumns = (columns: EventColumn[]) => {
    const normalized = normalizeVisibleEventColumns(columns);
    localStorage.setItem(COLUMNS_STORAGE_KEY, JSON.stringify(normalized));
    setVisibleColumns(normalized);
  };

  const exportEvents = async (format: 'csv' | 'json') => {
    try {
      const blob = await client.exportEvents(filters, format);
      const url = URL.createObjectURL(blob);
      const anchor = document.createElement('a');
      anchor.href = url;
      anchor.download = `gateway-usage-events.${format}`;
      anchor.click();
      URL.revokeObjectURL(url);
    } catch (exportError) {
      setError(exportError instanceof Error ? exportError.message : '导出失败');
    }
  };

  return (
    <section className={styles.content} data-od-id={`page-${activeTab}`}>
      <FilterBar draft={draftFilters} onChange={setDraftFilters} onApply={applyFilters} loading={loading} />
      {activeTab === 'overview' && (
        <div className={styles.granularityBar}>
          <span>时间粒度</span>
          {(['auto', 'hour', 'day'] as const).map((g) => (
            <button key={g} data-active={granularity === g} onClick={() => setGranularity(g)}>
              {g === 'auto' ? '自动' : g === 'hour' ? '小时' : '天'}
            </button>
          ))}
        </div>
      )}
      {error && <div className={styles.errorBanner} role="alert"><span>{error}</span><Button size="sm" variant="secondary" onClick={() => void loadActiveTab()}>重试</Button></div>}
      {loading && !error ? <div className={styles.loadingState} aria-busy="true">正在加载网关用量…</div> : (
        activeTab === 'overview'
          ? overview && <Overview data={overview} metric={trendMetric} onMetricChange={setTrendMetric} />
          : activeTab === 'analysis'
            ? analysisSummary && <Analysis breakdowns={breakdowns} summary={analysisSummary} />
            : <EventsTable events={events} hasMore={hasMore} loadingMore={loadingMore} onLoadMore={() => void loadMore()} visibleColumns={visibleColumns} onVisibleColumnsChange={changeVisibleColumns} onExport={(format) => void exportEvents(format)} client={client} />
      )}
    </section>
  );
}
