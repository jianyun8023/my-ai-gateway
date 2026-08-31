import { useCallback, useEffect, useMemo, useRef, useState, type CSSProperties } from 'react';
import { useVirtualizer } from '@tanstack/react-virtual';
import { Bar, Doughnut, Line } from 'react-chartjs-2';
import '@/lib/chartjs';
import { Button } from '@/components/ui/Button';
import { Card } from '@/components/ui/Card';
import { EmptyState } from '@/components/ui/EmptyState';
import { GatewayUsageClient, type GatewayUsageFilters, type UsageBreakdownDimension, type UsageBreakdownItem, type UsageEventViewModel, type UsageOverviewViewModel } from '@/gateway-usage';
import { useThemeStore } from '@/stores/useThemeStore';
import styles from './GatewayUsagePage.module.scss';

export const GATEWAY_USAGE_TABS = ['overview', 'analysis', 'events'] as const;
export type GatewayUsageTab = typeof GATEWAY_USAGE_TABS[number];

const TAB_LABELS: Record<GatewayUsageTab, string> = {
  overview: 'Overview',
  analysis: 'Analysis',
  events: 'Request Events',
};

const ADMIN_KEY_STORAGE_KEY = 'my-ai-gateway-admin-key-v1';
const FILTER_STORAGE_KEY = 'my-ai-gateway-usage-filters-v1';
const COLUMNS_STORAGE_KEY = 'my-ai-gateway-usage-event-columns-v1';

const EVENT_COLUMNS = [
  'time',
  'logicalModel',
  'upstreamModel',
  'provider',
  'sourceAccount',
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
  sourceAccount: 'Client Source / Account',
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

const safeSessionRead = (key: string): string => {
  try {
    return sessionStorage.getItem(key) ?? '';
  } catch {
    return '';
  }
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

export const resolveGatewayUsageTab = (hash: string): GatewayUsageTab => {
  const value = hash.replace(/^#\/?/, '');
  return GATEWAY_USAGE_TABS.includes(value as GatewayUsageTab) ? value as GatewayUsageTab : 'overview';
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

const makeChartColors = () => ['#5d7cfa', '#22b8a7', '#f0a44b', '#b77bf3', '#e76f83', '#64a7db', '#90a85b'];

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
      <label>Client Source<input value={draft.source ?? ''} onChange={(event) => update('source', event.target.value)} placeholder="全部" /></label>
      <label>Account<input value={draft.account ?? ''} onChange={(event) => update('account', event.target.value)} placeholder="全部" /></label>
      <label>入站协议<input value={draft.protocolIn ?? ''} onChange={(event) => update('protocolIn', event.target.value)} placeholder="全部" /></label>
      <label>上游协议<input value={draft.protocolUpstream ?? ''} onChange={(event) => update('protocolUpstream', event.target.value)} placeholder="全部" /></label>
      <label>Virtual Key ID<input inputMode="numeric" value={draft.virtualKey ?? ''} onChange={(event) => update('virtualKey', event.target.value)} placeholder="全部" /></label>
      <label>状态<select value={draft.status ?? ''} onChange={(event) => update('status', event.target.value)}><option value="">全部</option><option value="success">成功</option><option value="failure">失败</option></select></label>
      <label>Usage source<select value={draft.usageSource ?? ''} onChange={(event) => update('usageSource', event.target.value)}><option value="">全部</option><option value="upstream">upstream</option><option value="parsed">parsed</option><option value="estimated">estimated</option><option value="missing">missing</option></select></label>
      <Button className={styles.applyFilters} onClick={onApply} loading={loading}>应用筛选</Button>
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

function Overview({ data }: { data: UsageOverviewViewModel }) {
  const { summary } = data;
  const hasData = summary.logicalRequests > 0 || summary.tokens.total > 0;
  if (!hasData) {
    return <EmptyState title="当前范围暂无用量" description="调整时间范围或筛选条件后重试。Token 统计不依赖价格配置。" />;
  }
  const colors = makeChartColors();
  const trendData = {
    labels: data.timeseries.map((point) => formatBucket(point.bucket)),
    datasets: [
      { label: 'Total Token', data: data.timeseries.map((point) => point.tokens.total), borderColor: colors[0], backgroundColor: 'rgba(93,124,250,.14)', fill: true, tension: 0.35 },
      { label: '逻辑请求', data: data.timeseries.map((point) => point.logicalRequests), borderColor: colors[1], backgroundColor: colors[1], tension: 0.35, yAxisID: 'requests' },
    ],
  };
  const compositionData = {
    labels: ['Input', 'Output', 'Reasoning', 'Cached'],
    datasets: [{ data: [summary.tokens.input, summary.tokens.output, summary.tokens.reasoning, summary.tokens.cached], backgroundColor: colors.slice(0, 4), borderWidth: 0 }],
  };

  return (
    <div className={styles.stack}>
      <div className={styles.statsGrid}>
        <Stat label="逻辑请求" value={formatNumber(summary.logicalRequests)} hint={`${formatNumber(summary.upstreamAttempts)} 次上游尝试`} />
        <Stat label="成功率" value={`${(summary.successRate * 100).toFixed(1)}%`} hint={`${summary.failedRequests} 次失败`} tone={summary.failedRequests > 0 ? 'warning' : 'success'} />
        <Stat label="Total Token" value={formatNumber(summary.tokens.total)} hint="最终逻辑请求口径" />
        <Stat label="Input Token" value={formatNumber(summary.tokens.input)} />
        <Stat label="Output Token" value={formatNumber(summary.tokens.output)} />
        <Stat label="Reasoning Token" value={formatNumber(summary.tokens.reasoning)} />
        <Stat label="Cached Token" value={formatNumber(summary.tokens.cached)} />
        <Stat label="Fallback / 重试" value={formatNumber(summary.retries)} hint="不重复累计最终 Token" />
      </div>
      <div className={styles.chartGrid}>
        <Card title="Token 与请求趋势" subtitle="UTC 存储，按浏览器本地时区展示">
          <div className={styles.chartLarge}><Line data={trendData} options={{ responsive: true, maintainAspectRatio: false, interaction: { mode: 'index', intersect: false }, scales: { requests: { position: 'right', grid: { display: false } } } }} /></div>
        </Card>
        <Card title="Token 构成" subtitle="无价格配置时仍完整展示">
          <div className={styles.chartSmall}><Doughnut data={compositionData} options={{ responsive: true, maintainAspectRatio: false, cutout: '68%' }} /></div>
        </Card>
      </div>
      <Card title="最近活动" subtitle="仅显示请求元数据，不包含 prompt / response 正文">
        <div className={styles.recentList}>
          {data.recentEvents.map((event) => (
            <div key={`${event.id}:${event.createdAt}`}>
              <span className={styles.statusDot} data-success={event.success} />
              <time>{formatTime(event.createdAt)}</time>
              <strong>{event.logicalModel}</strong>
              <span>{event.provider} · {event.source} / {event.account}</span>
              <em>{formatNumber(event.tokens.total)} Token</em>
            </div>
          ))}
        </div>
      </Card>
    </div>
  );
}

const ANALYSIS_DIMENSIONS: Array<{ dimension: UsageBreakdownDimension; title: string }> = [
  { dimension: 'logical_model', title: 'Logical model' },
  { dimension: 'upstream_model', title: 'Upstream model' },
  { dimension: 'provider', title: 'Provider' },
  { dimension: 'source', title: 'Client Source' },
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

function Analysis({ breakdowns }: { breakdowns: Partial<Record<UsageBreakdownDimension, UsageBreakdownItem[]>> }) {
  const allRows = Object.values(breakdowns).flatMap((rows) => rows ?? []);
  const latencyRows = [...allRows].filter((row) => row.averageLatencyMs !== undefined).sort((a, b) => (b.averageLatencyMs ?? 0) - (a.averageLatencyMs ?? 0)).slice(0, 8);
  if (allRows.length === 0) return <EmptyState title="当前范围暂无分析数据" description="分布数据由网关 Usage breakdown API 提供。" />;
  return (
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
    case 'sourceAccount': return <span>{event.source}<small>{event.account}</small></span>;
    case 'protocol': return <span>{event.protocolIn}<small>→ {event.protocolUpstream}</small></span>;
    case 'status': return <span className={styles.statusBadge} data-success={event.success}>{event.statusCode || '—'} · {event.success ? '成功' : '失败'}</span>;
    case 'retries': return event.fallback ? `${event.retryCount} · fallback` : String(event.retryCount);
    case 'latency': return `${formatNumber(event.latencyMs)} ms`;
    case 'tokens': return formatNumber(event.tokens.total);
    case 'usageSource': return <UsageBadge source={event.usageSource} />;
  }
};

function EventDetails({ event, onClose }: { event: UsageEventViewModel; onClose: () => void }) {
  return (
    <div className={styles.drawerBackdrop} role="presentation" onMouseDown={(mouseEvent) => mouseEvent.target === mouseEvent.currentTarget && onClose()}>
      <aside className={styles.drawer} role="dialog" aria-modal="true" aria-label="请求事件详情">
        <header><div><span>Request Event</span><h2>{event.requestId}</h2></div><Button variant="ghost" onClick={onClose}>关闭</Button></header>
        <section className={styles.detailGrid}>
          <div><span>时间</span><strong>{formatTime(event.createdAt)}</strong></div>
          <div><span>状态</span><strong>{event.statusCode} · {event.success ? '成功' : '失败'}</strong></div>
          <div><span>Logical model</span><strong>{event.logicalModel}</strong></div>
          <div><span>Upstream model</span><strong>{event.upstreamModel}</strong></div>
          <div><span>Provider</span><strong>{event.provider}</strong></div>
          <div><span>Client Source / Account</span><strong>{event.source} / {event.account}</strong></div>
          <div><span>协议</span><strong>{event.protocolIn} → {event.protocolUpstream}</strong></div>
          <div><span>Usage source</span><strong><UsageBadge source={event.usageSource} /></strong></div>
          <div><span>延迟</span><strong>{formatNumber(event.latencyMs)} ms</strong></div>
          <div><span>重试</span><strong>{event.retryCount}{event.fallback ? ' · fallback' : ''}</strong></div>
        </section>
        <Card title="Token" subtitle="最终逻辑请求口径，不因 fallback 重复累计">
          <div className={styles.tokenDetails}><span>Input <strong>{event.tokens.input}</strong></span><span>Output <strong>{event.tokens.output}</strong></span><span>Reasoning <strong>{event.tokens.reasoning}</strong></span><span>Cached <strong>{event.tokens.cached}</strong></span><span>Total <strong>{event.tokens.total}</strong></span></div>
        </Card>
        <Card title="上游尝试" subtitle="失败尝试没有可确认 usage 时不会虚构 Token">
          {event.attempts.length === 0 ? <EmptyState title="没有独立 attempt 明细" description="事件仍保留最终账号与 retry_count。" /> : (
            <ol className={styles.attemptList}>{event.attempts.map((attempt) => <li key={attempt.attemptIndex}><span>#{attempt.attemptIndex + 1}</span><strong>{attempt.account}</strong><span>{attempt.provider} · {attempt.source}</span><span>{attempt.statusCode} · {attempt.latencyMs} ms</span></li>)}</ol>
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
}

function EventsTable({ events, hasMore, loadingMore, onLoadMore, visibleColumns, onVisibleColumnsChange, onExport }: EventsTableProps) {
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
    <Card variant="flush" title="请求事件" subtitle="稳定游标分页 · 虚拟滚动" extra={<div className={styles.eventActions}><details><summary>列偏好</summary><div className={styles.columnMenu}>{EVENT_COLUMNS.map((column) => <label key={column}><input type="checkbox" checked={visibleColumns.includes(column)} onChange={() => onVisibleColumnsChange(visibleColumns.includes(column) ? visibleColumns.filter((item) => item !== column) : EVENT_COLUMNS.filter((item) => visibleColumns.includes(item) || item === column))} />{EVENT_COLUMN_LABELS[column]}</label>)}</div></details><Button size="sm" variant="secondary" onClick={() => onExport('csv')}>导出 CSV</Button><Button size="sm" variant="secondary" onClick={() => onExport('json')}>导出 JSON</Button></div>}>
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
      {selectedEvent && <EventDetails event={selectedEvent} onClose={() => setSelectedEvent(undefined)} />}
    </Card>
  );
}

export function GatewayUsagePage() {
  const [activeTab, setActiveTab] = useState<GatewayUsageTab>(() => resolveGatewayUsageTab(window.location.hash));
  const [adminKey, setAdminKey] = useState(() => safeSessionRead(ADMIN_KEY_STORAGE_KEY));
  const adminKeyRef = useRef(adminKey);
  const clientRef = useRef(new GatewayUsageClient({ getAdminKey: () => adminKeyRef.current }));
  const [draftFilters, setDraftFilters] = useState<GatewayUsageFilters>(safeParseFilters);
  const [filters, setFilters] = useState<GatewayUsageFilters>(draftFilters);
  const [overview, setOverview] = useState<UsageOverviewViewModel>();
  const [breakdowns, setBreakdowns] = useState<Partial<Record<UsageBreakdownDimension, UsageBreakdownItem[]>>>({});
  const [events, setEvents] = useState<UsageEventViewModel[]>([]);
  const [nextCursor, setNextCursor] = useState<string>();
  const [hasMore, setHasMore] = useState(false);
  const [loading, setLoading] = useState(true);
  const [loadingMore, setLoadingMore] = useState(false);
  const [error, setError] = useState('');
  const [visibleColumns, setVisibleColumns] = useState<EventColumn[]>(loadVisibleColumns);
  const theme = useThemeStore((state) => state.theme);
  const setTheme = useThemeStore((state) => state.setTheme);

  const loadActiveTab = useCallback(async (signal?: AbortSignal) => {
    setLoading(true);
    setError('');
    try {
      if (activeTab === 'overview') {
        setOverview(await clientRef.current.overview(filters, signal));
      } else if (activeTab === 'analysis') {
        const results = await Promise.all(ANALYSIS_DIMENSIONS.map(async ({ dimension }) => [
          dimension,
          await clientRef.current.breakdown(filters, dimension, signal),
        ] as const));
        setBreakdowns(Object.fromEntries(results));
      } else {
        const page = await clientRef.current.events({ filters, limit: 100 }, signal);
        setEvents(page.events);
        setNextCursor(page.nextCursor);
        setHasMore(page.hasMore);
      }
    } catch (loadError) {
      if (signal?.aborted) return;
      setError(loadError instanceof Error ? loadError.message : '加载用量数据失败');
    } finally {
      if (!signal?.aborted) setLoading(false);
    }
  }, [activeTab, filters]);

  useEffect(() => {
    const onHashChange = () => setActiveTab(resolveGatewayUsageTab(window.location.hash));
    window.addEventListener('hashchange', onHashChange);
    return () => window.removeEventListener('hashchange', onHashChange);
  }, []);

  useEffect(() => {
    const controller = new AbortController();
    void loadActiveTab(controller.signal);
    return () => controller.abort();
  }, [loadActiveTab]);

  const navigate = (tab: GatewayUsageTab) => {
    window.location.hash = tab;
    setActiveTab(tab);
  };

  const applyFilters = () => {
    if (new Date(draftFilters.from) >= new Date(draftFilters.to)) {
      setError('开始时间必须早于结束时间');
      return;
    }
    localStorage.setItem(FILTER_STORAGE_KEY, JSON.stringify(draftFilters));
    setFilters({ ...draftFilters });
  };

  const saveAdminKey = () => {
    adminKeyRef.current = adminKey;
    if (adminKey) sessionStorage.setItem(ADMIN_KEY_STORAGE_KEY, adminKey);
    else sessionStorage.removeItem(ADMIN_KEY_STORAGE_KEY);
    void loadActiveTab();
  };

  const loadMore = useCallback(async () => {
    if (!nextCursor || !hasMore || loadingMore) return;
    setLoadingMore(true);
    try {
      const page = await clientRef.current.events({ filters, cursor: nextCursor, limit: 100 });
      setEvents((current) => appendStableEventPage(current, page.events));
      setNextCursor(page.nextCursor);
      setHasMore(page.hasMore);
    } catch (loadError) {
      setError(loadError instanceof Error ? loadError.message : '加载更多事件失败');
    } finally {
      setLoadingMore(false);
    }
  }, [filters, hasMore, loadingMore, nextCursor]);

  const changeVisibleColumns = (columns: EventColumn[]) => {
    const normalized = normalizeVisibleEventColumns(columns);
    localStorage.setItem(COLUMNS_STORAGE_KEY, JSON.stringify(normalized));
    setVisibleColumns(normalized);
  };

  const exportEvents = async (format: 'csv' | 'json') => {
    try {
      const blob = await clientRef.current.exportEvents(filters, format);
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

  const localTimeZone = useMemo(() => Intl.DateTimeFormat().resolvedOptions().timeZone, []);

  return (
    <div className={styles.page}>
      <header className={styles.topbar}>
        <div className={styles.brand}><span>G</span><div><strong>my-ai-gateway</strong><small>Usage Console</small></div></div>
        <nav aria-label="主导航">{GATEWAY_USAGE_TABS.map((tab) => <button key={tab} data-active={activeTab === tab} onClick={() => navigate(tab)}>{TAB_LABELS[tab]}</button>)}</nav>
        <div className={styles.headerActions}>
          <label className={styles.keyInput}>Admin Key<input type="password" value={adminKey} onChange={(event) => setAdminKey(event.target.value)} placeholder="未配置时可留空" /><Button size="sm" variant="secondary" onClick={saveAdminKey}>应用</Button></label>
          <Button size="sm" variant="ghost" onClick={() => setTheme(theme === 'dark' ? 'white' : 'dark')}>{theme === 'dark' ? '浅色' : '深色'}</Button>
          <Button size="sm" onClick={() => void loadActiveTab()} loading={loading}>刷新</Button>
        </div>
      </header>
      <main className={styles.main}>
        <section className={styles.pageHeading}><div><span>PostgreSQL Usage Events</span><h1>{TAB_LABELS[activeTab]}</h1><p>逻辑请求与上游尝试分开统计 · 本地时区：{localTimeZone}</p></div><aside><strong>UTC</strong><span>存储边界</span></aside></section>
        <FilterBar draft={draftFilters} onChange={setDraftFilters} onApply={applyFilters} loading={loading} />
        {error && <div className={styles.errorBanner} role="alert"><span>{error}</span><Button size="sm" variant="secondary" onClick={() => void loadActiveTab()}>重试</Button></div>}
        {loading && !error ? <div className={styles.loadingState} aria-busy="true">正在加载网关用量…</div> : (
          activeTab === 'overview'
            ? overview && <Overview data={overview} />
            : activeTab === 'analysis'
              ? <Analysis breakdowns={breakdowns} />
              : <EventsTable events={events} hasMore={hasMore} loadingMore={loadingMore} onLoadMore={() => void loadMore()} visibleColumns={visibleColumns} onVisibleColumnsChange={changeVisibleColumns} onExport={(format) => void exportEvents(format)} />
        )}
      </main>
      <footer className={styles.footer}>my-ai-gateway · UI interactions adapted from CPA Usage Keeper under the MIT License</footer>
    </div>
  );
}
