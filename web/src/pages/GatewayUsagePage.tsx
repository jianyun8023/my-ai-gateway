import { useCallback, useEffect, useRef, useState, type CSSProperties } from 'react';
import { useTranslation } from 'react-i18next';
import type { TFunction } from 'i18next';
import { useVirtualizer } from '@tanstack/react-virtual';
import { Bar, Doughnut, Line } from 'react-chartjs-2';
import '@/lib/chartjs';
import { Button } from '@/components/ui/Button';
import { Card } from '@/components/ui/Card';
import { EmptyState } from '@/components/ui/EmptyState';
import { GatewayUsageClient, type GatewayUsageFilters, type UsageBreakdownDimension, type UsageBreakdownItem, type UsageEventViewModel, type UsageOverviewViewModel, type UsageSummaryViewModel } from '@/gateway-usage';
import type { GatewayUsageTab } from '@/lib/consoleNavigation';
import { useLocalizedApiError } from '@/hooks/useLocalizedApiError';
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
  time: 'usage.field.time',
  logicalModel: 'usage.field.logical_model',
  upstreamModel: 'usage.field.upstream_model',
  provider: 'usage.field.provider',
  sourceAccount: 'usage.field.source_account',
  clientSource: 'usage.field.client_source',
  protocol: 'usage.field.protocol',
  status: 'usage.field.status',
  retries: 'usage.field.retries',
  latency: 'usage.field.latency',
  tokens: 'usage.field.tokens',
  usageSource: 'usage.field.usage_source',
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
  const { t } = useTranslation('console');
  const update = (field: keyof GatewayUsageFilters, value: string) => onChange({
    ...draft,
    [field]: value || undefined,
  });
  const setPreset = (durationMs: number) => {
    const to = new Date();
    onChange({ ...draft, from: new Date(to.getTime() - durationMs).toISOString(), to: to.toISOString() });
  };

  return (
    <section className={styles.filters} aria-label={t('usage.filter.aria')}>
      <div className={styles.filterPresets}>
        <Button size="sm" variant="ghost" onClick={() => setPreset(DAY_MS)}>{t('usage.filter.preset_24h')}</Button>
        <Button size="sm" variant="ghost" onClick={() => setPreset(7 * DAY_MS)}>{t('usage.filter.preset_7d')}</Button>
        <Button size="sm" variant="ghost" onClick={() => setPreset(30 * DAY_MS)}>{t('usage.filter.preset_30d')}</Button>
      </div>
      <label>{t('usage.filter.from')}<input type="datetime-local" value={toLocalInputValue(draft.from)} onChange={(event) => update('from', fromLocalInputValue(event.target.value))} /></label>
      <label>{t('usage.filter.to')}<input type="datetime-local" value={toLocalInputValue(draft.to)} onChange={(event) => update('to', fromLocalInputValue(event.target.value))} /></label>
      <label>{t('usage.field.logical_model')}<input value={draft.logicalModel ?? ''} onChange={(event) => update('logicalModel', event.target.value)} placeholder={t('common.all')} /></label>
      <label>{t('usage.field.upstream_model')}<input value={draft.upstreamModel ?? ''} onChange={(event) => update('upstreamModel', event.target.value)} placeholder={t('common.all')} /></label>
      <label>{t('usage.field.provider')}<input value={draft.provider ?? ''} onChange={(event) => update('provider', event.target.value)} placeholder={t('common.all')} /></label>
      <label>{t('usage.field.source_id')}<input value={draft.sourceId ?? ''} onChange={(event) => update('sourceId', event.target.value)} placeholder={t('common.all')} /></label>
      <label>{t('usage.field.client_source')}<input value={draft.clientSource ?? ''} onChange={(event) => update('clientSource', event.target.value)} placeholder={t('common.all')} /></label>
      <label>{t('usage.field.account')}<input value={draft.account ?? ''} onChange={(event) => update('account', event.target.value)} placeholder={t('common.all')} /></label>
      <label>{t('usage.field.protocol_in')}<input value={draft.protocolIn ?? ''} onChange={(event) => update('protocolIn', event.target.value)} placeholder={t('common.all')} /></label>
      <label>{t('usage.field.protocol_upstream')}<input value={draft.protocolUpstream ?? ''} onChange={(event) => update('protocolUpstream', event.target.value)} placeholder={t('common.all')} /></label>
      <label>{t('usage.field.virtual_key_id')}<input inputMode="numeric" value={draft.virtualKey ?? ''} onChange={(event) => update('virtualKey', event.target.value)} placeholder={t('common.all')} /></label>
      <label>{t('usage.field.status')}<select value={draft.status ?? ''} onChange={(event) => update('status', event.target.value)}><option value="">{t('common.all')}</option><option value="success">{t('usage.filter.status_success')}</option><option value="failure">{t('usage.filter.status_failure')}</option></select></label>
      <label>{t('usage.field.usage_source')}<select value={draft.usageSource ?? ''} onChange={(event) => update('usageSource', event.target.value)}><option value="">{t('common.all')}</option><option value="upstream">{t('usage.usage_source.upstream')}</option><option value="parsed">{t('usage.usage_source.parsed')}</option><option value="estimated">{t('usage.usage_source.estimated')}</option><option value="missing">{t('usage.usage_source.missing')}</option></select></label>
      <Button className={styles.applyFilters} variant="secondary" onClick={onApply} loading={loading}>{t('usage.filter.apply')}</Button>
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

const TREND_METRICS: Array<{ value: TrendMetric; labelKey: string }> = [
  { value: 'total', labelKey: 'usage.metric.total' },
  { value: 'input', labelKey: 'usage.metric.input' },
  { value: 'output', labelKey: 'usage.metric.output' },
  { value: 'reasoning', labelKey: 'usage.metric.reasoning' },
  { value: 'cached', labelKey: 'usage.metric.cached' },
  { value: 'requests', labelKey: 'usage.metric.requests' },
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
  const { t } = useTranslation('console');
  const colors = makeChartColors();
  const rows = [
    { labelKey: 'usage.legend.input', value: summary.tokens.input, color: colors[0] },
    { labelKey: 'usage.legend.output', value: summary.tokens.output, color: colors[1] },
    { labelKey: 'usage.legend.reasoning', value: summary.tokens.reasoning, color: colors[2] },
    { labelKey: 'usage.legend.cached', value: summary.tokens.cached, color: colors[3] },
  ] as const;
  const compositionData = {
    labels: rows.map((row) => t(row.labelKey)),
    datasets: [{ data: rows.map((row) => row.value), backgroundColor: rows.map((row) => row.color), borderWidth: 0 }],
  };

  return (
    <Card title={t('usage.composition.title')} subtitle={t('usage.composition.subtitle')}>
      <div className={styles.tokenComposition}>
        <div className={styles.chartSmall}><Doughnut data={compositionData} options={{ responsive: true, maintainAspectRatio: false, cutout: '70%', plugins: { legend: { display: false } } }} /></div>
        <dl>{rows.map((row) => <div key={row.labelKey}><dt><i style={{ background: row.color }} />{t(row.labelKey)}</dt><dd>{formatNumber(row.value)}</dd></div>)}</dl>
      </div>
    </Card>
  );
}

function ModelDistribution({ rows }: { rows: UsageBreakdownItem[] }) {
  const { t } = useTranslation('console');
  const visibleRows = rows.slice(0, 6);
  const maxTokens = Math.max(...visibleRows.map((row) => row.tokens.total), 1);

  return (
    <Card title={t('usage.distribution.title')} subtitle={t('usage.distribution.subtitle')}>
      {visibleRows.length === 0 ? <EmptyState title={t('usage.distribution.empty')} /> : (
        <div className={styles.distributionList}>{visibleRows.map((row) => (
          <div key={row.key}>
            <div><strong>{row.label}</strong><span>{t('usage.value.requests_tokens', { count: formatNumber(row.logicalRequests), tokens: formatNumber(row.tokens.total) })}</span></div>
            <span className={styles.distributionTrack}><i style={{ width: `${Math.max(4, (row.tokens.total / maxTokens) * 100)}%` }} /></span>
          </div>
        ))}</div>
      )}
    </Card>
  );
}

function Overview({ data, metric, onMetricChange }: { data: UsageOverviewViewModel; metric: TrendMetric; onMetricChange: (m: TrendMetric) => void }) {
  const { t } = useTranslation('console');
  const { summary } = data;
  const hasData = summary.logicalRequests > 0 || summary.tokens.total > 0;
  if (!hasData) {
    return <EmptyState title={t('usage.empty.overview_title')} description={t('usage.empty.overview_desc')} />;
  }
  const colors = makeChartColors();
  const metricLabel = t(TREND_METRICS.find((m) => m.value === metric)?.labelKey ?? 'usage.metric.total');
  const secondaryMetric = metric === 'requests' ? 'total' : 'requests';
  const secondaryLabel = t(TREND_METRICS.find((m) => m.value === secondaryMetric)?.labelKey ?? 'usage.metric.total');
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
        <Stat label={t('usage.stat.logical_requests')} value={formatNumber(summary.logicalRequests)} hint={t('usage.stat.upstream_attempts', { count: formatNumber(summary.upstreamAttempts), retries: formatNumber(summary.retries) })} />
        <Stat label={t('usage.stat.success_rate')} value={`${(summary.successRate * 100).toFixed(1)}%`} hint={t('usage.stat.failures', { count: formatNumber(summary.failedRequests) })} tone={summary.failedRequests > 0 ? 'warning' : 'success'} />
        <Stat label={t('usage.stat.total_tokens')} value={formatNumber(summary.tokens.total)} hint={t('usage.stat.final_accounting')} />
        <Stat label={t('usage.stat.avg_latency')} value={formatDuration(summary.averageLatencyMs)} hint={t('usage.stat.p95', { value: formatDuration(summary.p95LatencyMs) })} />
      </div>
      <div className={styles.chartGrid}>
        <Card title={t('usage.trend.title')} subtitle={t('usage.trend.subtitle')} data-od-id="token-trend" extra={
          <select value={metric} onChange={(e) => onMetricChange(e.target.value as TrendMetric)} className={styles.metricSelect}>
            {TREND_METRICS.map((m) => <option key={m.value} value={m.value}>{t(m.labelKey)}</option>)}
          </select>
        }>
          <div className={styles.chartLarge}><Line data={trendData} options={{ responsive: true, maintainAspectRatio: false, interaction: { mode: 'index', intersect: false }, scales: { secondary: { position: 'right', grid: { display: false } } } }} /></div>
        </Card>
        <TokenComposition summary={summary} />
      </div>
      <div className={styles.overviewLowerGrid}>
        <Card title={t('usage.recent.title')} subtitle={t('usage.recent.subtitle')} data-od-id="recent-activity">
          {data.recentEvents.length === 0 ? <EmptyState title={t('usage.recent.empty')} /> : (
            <div className={styles.recentList}>{data.recentEvents.map((event) => (
              <div key={`${event.id}:${event.createdAt}`}>
                <span className={styles.statusDot} data-success={event.success} />
                <time>{formatTime(event.createdAt)}</time>
                <strong>{event.logicalModel}</strong>
                <span>{event.provider} · {event.sourceId} / {event.account}</span>
                <em>{t('usage.value.tokens', { count: formatNumber(event.tokens.total) })}</em>
              </div>
            ))}</div>
          )}
        </Card>
        <div data-od-id="model-distribution"><ModelDistribution rows={data.logicalModels} /></div>
      </div>
    </div>
  );
}

const ANALYSIS_DIMENSIONS: Array<{ dimension: UsageBreakdownDimension; titleKey: string }> = [
  { dimension: 'logical_model', titleKey: 'usage.field.logical_model' },
  { dimension: 'upstream_model', titleKey: 'usage.field.upstream_model' },
  { dimension: 'provider', titleKey: 'usage.field.provider' },
  { dimension: 'source_id', titleKey: 'usage.field.source_id' },
  { dimension: 'client_source', titleKey: 'usage.field.client_source' },
  { dimension: 'account', titleKey: 'usage.field.account' },
  { dimension: 'protocol_in', titleKey: 'usage.field.protocol_in' },
  { dimension: 'protocol_upstream', titleKey: 'usage.field.protocol_upstream' },
];

function BreakdownChart({ title, rows }: { title: string; rows: UsageBreakdownItem[] }) {
  const { t } = useTranslation('console');
  const visibleRows = rows.slice(0, 8);
  return (
    <Card title={title} subtitle={t('usage.breakdown.subtitle')}>
      {visibleRows.length === 0 ? <EmptyState title={t('usage.breakdown.empty')} /> : (
        <div className={styles.breakdownChart}>
          <Bar data={{ labels: visibleRows.map((item) => item.label), datasets: [{ label: t('usage.legend.total'), data: visibleRows.map((item) => item.tokens.total), backgroundColor: makeChartColors()[0], borderRadius: 6 }] }} options={{ indexAxis: 'y', responsive: true, maintainAspectRatio: false, plugins: { legend: { display: false } } }} />
        </div>
      )}
    </Card>
  );
}

function Analysis({ breakdowns, summary }: { breakdowns: Partial<Record<UsageBreakdownDimension, UsageBreakdownItem[]>>; summary: UsageSummaryViewModel }) {
  const { t } = useTranslation('console');
  const allRows = Object.values(breakdowns).flatMap((rows) => rows ?? []);
  const latencyRows = [...allRows].filter((row) => row.averageLatencyMs !== undefined).sort((a, b) => (b.averageLatencyMs ?? 0) - (a.averageLatencyMs ?? 0)).slice(0, 8);
  if (allRows.length === 0 && summary.tokens.total === 0) return <EmptyState title={t('usage.empty.analysis_title')} description={t('usage.empty.analysis_desc')} />;
  return (
    <div className={styles.stack}>
      <TokenComposition summary={summary} />
      <div className={styles.analysisGrid}>
        {ANALYSIS_DIMENSIONS.map(({ dimension, titleKey }) => <BreakdownChart key={dimension} title={t(titleKey)} rows={breakdowns[dimension] ?? []} />)}
        <Card title={t('usage.latency.title')} subtitle={t('usage.latency.subtitle')}>
          {latencyRows.length === 0 ? <EmptyState title={t('usage.latency.empty')} /> : (
            <div className={styles.breakdownChart}>
              <Bar data={{ labels: latencyRows.map((item) => item.label), datasets: [{ label: t('usage.latency.dataset'), data: latencyRows.map((item) => item.averageLatencyMs ?? 0), backgroundColor: makeChartColors()[2], borderRadius: 6 }] }} options={{ responsive: true, maintainAspectRatio: false, plugins: { legend: { display: false } } }} />
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

const renderEventCell = (event: UsageEventViewModel, column: EventColumn, t: TFunction) => {
  switch (column) {
    case 'time': return <time>{formatTime(event.createdAt)}</time>;
    case 'logicalModel': return <strong>{event.logicalModel}</strong>;
    case 'upstreamModel': return event.upstreamModel;
    case 'provider': return event.provider;
    case 'sourceAccount': return <span>{event.sourceId}<small>{event.account}</small></span>;
    case 'clientSource': return event.clientSource;
    case 'protocol': return <span>{event.protocolIn}<small>→ {event.protocolUpstream}</small></span>;
    case 'status': return <span className={styles.statusBadge} data-success={event.success}>{event.statusCode || '—'} · {event.success ? t('usage.event.success') : t('usage.event.failure')}</span>;
    case 'retries': return event.fallback ? t('usage.event.retries_fallback', { count: event.retryCount }) : String(event.retryCount);
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
  const { t } = useTranslation('console');
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
      <aside className={styles.drawer} role="dialog" aria-modal="true" aria-label={t('usage.detail.aria')} data-od-id="event-drawer">
        <header><div><span>{t('usage.detail.kicker')}</span><h2>{event.requestId}</h2></div><Button variant="ghost" onClick={onClose}>{t('common.close')}</Button></header>
        <section className={styles.detailGrid}>
          <div><span>{t('usage.field.time')}</span><strong>{formatTime(event.createdAt)}</strong></div>
          <div><span>{t('usage.field.status')}</span><strong>{event.statusCode} · {event.success ? t('usage.event.success') : t('usage.event.failure')}</strong></div>
          <div><span>{t('usage.field.logical_model')}</span><strong>{event.logicalModel}</strong></div>
          <div><span>{t('usage.field.upstream_model')}</span><strong>{event.upstreamModel}</strong></div>
          <div><span>{t('usage.field.provider')}</span><strong>{event.provider}</strong></div>
          <div><span>{t('usage.field.source_id')}</span><strong>{event.sourceId}</strong></div>
          <div><span>{t('usage.field.client_source')}</span><strong>{event.clientSource}</strong></div>
          <div><span>{t('usage.field.account')}</span><strong>{event.account}</strong></div>
          <div><span>{t('usage.field.protocol')}</span><strong>{event.protocolIn} → {event.protocolUpstream}</strong></div>
          <div><span>{t('usage.field.usage_source')}</span><strong><UsageBadge source={event.usageSource} /></strong></div>
          <div><span>{t('usage.field.latency')}</span><strong>{formatNumber(event.latencyMs)} ms</strong></div>
          <div><span>{t('usage.field.retries')}</span><strong>{event.fallback ? t('usage.event.retries_fallback', { count: event.retryCount }) : String(event.retryCount)}</strong></div>
        </section>
        <Card title={t('usage.detail.token_title')} subtitle={t('usage.detail.token_subtitle')}>
          <div className={styles.tokenDetails}><span>{t('usage.legend.input')} <strong>{event.tokens.input}</strong></span><span>{t('usage.legend.output')} <strong>{event.tokens.output}</strong></span><span>{t('usage.legend.reasoning')} <strong>{event.tokens.reasoning}</strong></span><span>{t('usage.legend.cached')} <strong>{event.tokens.cached}</strong></span><span>{t('usage.legend.total')} <strong>{event.tokens.total}</strong></span></div>
        </Card>
        <Card title={t('usage.detail.attempts_title')} subtitle={t('usage.detail.attempts_subtitle')}>
          {loadingAttempts ? <div style={{ padding: '1rem', opacity: 0.6 }}>{t('usage.detail.attempts_loading')}</div> : displayAttempts.length === 0 ? <EmptyState title={t('usage.detail.attempts_empty_title')} description={t('usage.detail.attempts_empty_desc')} /> : (
            <ol className={styles.attemptList}>{displayAttempts.map((attempt) => <li key={attempt.attemptNo}><span>#{attempt.attemptNo + 1}</span><strong>{attempt.account}</strong><span>{attempt.sourceId} · {attempt.provider} · {attempt.upstreamModel}</span><span className={attempt.success ? styles.statusSuccess : styles.statusFailure}>{attempt.statusCode} · {attempt.latencyMs} ms</span></li>)}</ol>
          )}
        </Card>
        {event.errorSummary && <Card title={t('usage.detail.error_summary')}><p className={styles.errorSummary}>{event.errorSummary}</p></Card>}
        <p className={styles.noBodyNotice}>{t('usage.detail.no_body_notice')}</p>
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
  const { t } = useTranslation('console');
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

  if (events.length === 0) return <EmptyState title={t('usage.events.empty_title')} description={t('usage.events.empty_desc')} />;

  return (
    <Card variant="flush" title={t('usage.events.title')} subtitle={t('usage.events.subtitle')} data-od-id="events-table" extra={<div className={styles.eventActions}><details><summary>{t('common.column_prefs')}</summary><div className={styles.columnMenu}>{EVENT_COLUMNS.map((column) => <label key={column}><input type="checkbox" checked={visibleColumns.includes(column)} onChange={() => onVisibleColumnsChange(visibleColumns.includes(column) ? visibleColumns.filter((item) => item !== column) : EVENT_COLUMNS.filter((item) => visibleColumns.includes(item) || item === column))} />{t(EVENT_COLUMN_LABELS[column])}</label>)}</div></details><Button size="sm" variant="secondary" onClick={() => onExport('csv')}>{t('common.export_csv')}</Button><Button size="sm" variant="secondary" onClick={() => onExport('json')}>{t('common.export_json')}</Button></div>}>
      <div className={styles.eventTable} style={{ '--event-columns': visibleColumns.length } as CSSProperties}>
        <div className={styles.eventHeader}>{visibleColumns.map((column) => <span key={column}>{t(EVENT_COLUMN_LABELS[column])}</span>)}</div>
        <div ref={parentRef} className={styles.eventScroll}>
          <div style={{ height: virtualizer.getTotalSize(), position: 'relative' }}>
            {virtualItems.map((virtualRow) => {
              const event = events[virtualRow.index];
              return (
                <button key={`${event.id}:${event.createdAt}`} className={styles.eventRow} style={{ transform: `translateY(${virtualRow.start}px)` }} onClick={() => setSelectedEvent(event)}>
                  {visibleColumns.map((column) => <span key={column}>{renderEventCell(event, column, t)}</span>)}
                </button>
              );
            })}
          </div>
          {loadingMore && <div className={styles.loadingMore}>{t('common.load_more')}</div>}
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
  const { t } = useTranslation('console');
  const localizeError = useLocalizedApiError();
  // 结构化接口错误(status/code)走 useLocalizedApiError 映射;无元数据的普通错误回退到页面业务文案。
  const toLocalizedError = useCallback((error: unknown, fallback: string): string => {
    const mapped = localizeError(error);
    return mapped === t('errors.unknown') ? fallback : mapped;
  }, [localizeError, t]);
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
      setError(toLocalizedError(loadError, t('usage.error.load_failed')));
    } finally {
      if (!signal?.aborted) {
        setLoading(false);
        onLoadingChange?.(false);
      }
    }
  }, [activeTab, client, filters, granularity, onLoadingChange, t, toLocalizedError]);

  useEffect(() => {
    const controller = new AbortController();
    void loadActiveTab(controller.signal);
    return () => controller.abort();
  }, [loadActiveTab, refreshRevision]);

  useEffect(() => () => onLoadingChange?.(false), [onLoadingChange]);

  const applyFilters = () => {
    if (new Date(draftFilters.from) >= new Date(draftFilters.to)) {
      setError(t('usage.error.invalid_range'));
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
      setError(toLocalizedError(loadError, t('usage.error.load_more_failed')));
    } finally {
      setLoadingMore(false);
    }
  }, [client, filters, hasMore, loadingMore, nextCursor, t, toLocalizedError]);

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
      setError(toLocalizedError(exportError, t('errors.export_failed')));
    }
  };

  return (
    <section className={styles.content} data-od-id={`page-${activeTab}`}>
      <FilterBar draft={draftFilters} onChange={setDraftFilters} onApply={applyFilters} loading={loading} />
      {activeTab === 'overview' && (
        <div className={styles.granularityBar}>
          <span>{t('usage.granularity.label')}</span>
          {(['auto', 'hour', 'day'] as const).map((g) => (
            <button key={g} data-active={granularity === g} onClick={() => setGranularity(g)}>
              {g === 'auto' ? t('usage.granularity.auto') : g === 'hour' ? t('usage.granularity.hour') : t('usage.granularity.day')}
            </button>
          ))}
        </div>
      )}
      {error && <div className={styles.errorBanner} role="alert"><span>{error}</span><Button size="sm" variant="secondary" onClick={() => void loadActiveTab()}>{t('common.retry')}</Button></div>}
      {loading && !error ? <div className={styles.loadingState} aria-busy="true">{t('usage.page.loading')}</div> : (
        activeTab === 'overview'
          ? overview && <Overview data={overview} metric={trendMetric} onMetricChange={setTrendMetric} />
          : activeTab === 'analysis'
            ? analysisSummary && <Analysis breakdowns={breakdowns} summary={analysisSummary} />
            : <EventsTable events={events} hasMore={hasMore} loadingMore={loadingMore} onLoadMore={() => void loadMore()} visibleColumns={visibleColumns} onVisibleColumnsChange={changeVisibleColumns} onExport={(format) => void exportEvents(format)} client={client} />
      )}
    </section>
  );
}
