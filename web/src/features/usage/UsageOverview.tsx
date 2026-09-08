import { Card } from '@/components/ui/Card';
import { EmptyState } from '@/components/ui/EmptyState';
import { TokenComposition } from '@/features/usage/TokenComposition';
import styles from '@/features/usage/Usage.module.scss';
import { lineChartOptions, makeChartColors } from '@/features/usage/charts';
import { formatBucket, formatDuration, formatTime } from '@/features/usage/formatters';
import { type TrendMetric } from '@/features/usage/model';
import { type UsageBreakdownItem, type UsageOverviewViewModel } from '@/gateway-usage';
import '@/lib/chartjs';
import { formatCompact } from '@/utils/formatCompact';
import { Line } from 'react-chartjs-2';
import { useTranslation } from 'react-i18next';

function Stat({ label, value, hint, tone }: { label: string; value: string; hint?: string; tone?: 'success' | 'warning' }) {
  return (
    <div className={styles.stat} data-tone={tone}>
      <span>{label}</span>
      <strong>{value}</strong>
      {hint && <small>{hint}</small>}
    </div>
  );
}

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

function ModelDistribution({ rows }: { rows: UsageBreakdownItem[] }) {
  const { t } = useTranslation('console');
  const visibleRows = rows.slice(0, 6);
  const maxTokens = Math.max(...visibleRows.map((row) => row.tokens.total), 1);

  return (
    <Card title={t('usage.distribution.title')} subtitle={t('usage.distribution.subtitle')}>
      {visibleRows.length === 0 ? <EmptyState title={t('usage.distribution.empty')} /> : (
        <div className={styles.distributionList}>{visibleRows.map((row) => (
          <div key={row.key}>
            <div><strong>{row.label}</strong><span>{t('usage.value.requests_tokens', { count: formatCompact(row.logicalRequests), tokens: formatCompact(row.tokens.total) })}</span></div>
            <span className={styles.distributionTrack}><i style={{ width: `${Math.max(4, (row.tokens.total / maxTokens) * 100)}%` }} /></span>
          </div>
        ))}</div>
      )}
    </Card>
  );
}

export function Overview({ data, metric, onMetricChange }: { data: UsageOverviewViewModel; metric: TrendMetric; onMetricChange: (m: TrendMetric) => void }) {
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
        <Stat label={t('usage.stat.logical_requests')} value={formatCompact(summary.logicalRequests)} hint={t('usage.stat.upstream_attempts', { count: formatCompact(summary.upstreamAttempts), retries: formatCompact(summary.retries) })} />
        <Stat label={t('usage.stat.success_rate')} value={`${(summary.successRate * 100).toFixed(1)}%`} hint={t('usage.stat.failures', { count: formatCompact(summary.failedRequests) })} tone={summary.failedRequests > 0 ? 'warning' : 'success'} />
        <Stat label={t('usage.stat.total_tokens')} value={formatCompact(summary.tokens.total)} hint={t('usage.stat.final_accounting')} />
        <Stat label={t('usage.stat.avg_latency')} value={formatDuration(summary.averageLatencyMs)} hint={t('usage.stat.p95', { value: formatDuration(summary.p95LatencyMs) })} />
      </div>
      <div className={styles.chartGrid}>
        <Card title={t('usage.trend.title')} subtitle={t('usage.trend.subtitle')} data-od-id="token-trend" extra={
          <select value={metric} onChange={(e) => onMetricChange(e.target.value as TrendMetric)} className={styles.metricSelect}>
            {TREND_METRICS.map((m) => <option key={m.value} value={m.value}>{t(m.labelKey)}</option>)}
          </select>
        }>
          <div className={styles.chartLarge}><Line data={trendData} options={lineChartOptions} /></div>
        </Card>
        <TokenComposition summary={summary} />
      </div>
      <div className={styles.overviewLowerGrid}>
        <Card title={t('usage.recent.title')} data-od-id="recent-activity">
          {data.recentEvents.length === 0 ? <EmptyState title={t('usage.recent.empty')} /> : (
            <div className={styles.recentList}>{data.recentEvents.map((event) => (
              <div key={`${event.id}:${event.createdAt}`}>
                <span className={styles.statusDot} data-success={event.success} />
                <time>{formatTime(event.createdAt)}</time>
                <strong>{event.logicalModel}</strong>
                <span>{event.provider} · {event.sourceId} / {event.account}</span>
                <em>{t('usage.value.tokens', { count: formatCompact(event.tokens.total) })}</em>
              </div>
            ))}</div>
          )}
        </Card>
        <div data-od-id="model-distribution"><ModelDistribution rows={data.logicalModels} /></div>
      </div>
    </div>
  );
}
