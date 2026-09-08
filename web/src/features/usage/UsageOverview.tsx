import { UsageTrend } from './UsageTrend';
import { TokenDistribution } from './TokenDistribution';
import { Card } from '@/components/ui/Card';
import { EmptyState } from '@/components/ui/EmptyState';
import { TokenComposition } from '@/features/usage/TokenComposition';
import styles from '@/features/usage/Usage.module.scss';
import { formatDuration, formatTime } from '@/features/usage/formatters';
import { type TrendMetric } from '@/features/usage/model';
import { type UsageOverviewViewModel } from '@/gateway-usage';
import { formatCompact } from '@/utils/formatCompact';
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

export function Overview({ data, metric, onMetricChange }: { data: UsageOverviewViewModel; metric: TrendMetric; onMetricChange: (m: TrendMetric) => void }) {
  const { t } = useTranslation('console');
  const { summary } = data;
  const hasData = summary.logicalRequests > 0 || summary.tokens.total > 0;
  if (!hasData) {
    return <EmptyState title={t('usage.empty.overview_title')} description={t('usage.empty.overview_desc')} />;
  }
  return (
    <div className={styles.stack}>
      <div className={styles.statsGrid} data-od-id="kpi-row">
        <Stat label={t('usage.stat.logical_requests')} value={formatCompact(summary.logicalRequests)} hint={t('usage.stat.upstream_attempts', { count: formatCompact(summary.upstreamAttempts), retries: formatCompact(summary.retries) })} />
        <Stat label={t('usage.stat.success_rate')} value={`${(summary.successRate * 100).toFixed(1)}%`} hint={t('usage.stat.failures', { count: formatCompact(summary.failedRequests) })} tone={summary.failedRequests > 0 ? 'warning' : 'success'} />
        <Stat label={t('usage.stat.total_tokens')} value={formatCompact(summary.tokens.total)} hint={t('usage.stat.final_accounting')} />
        <Stat label={t('usage.stat.avg_latency')} value={formatDuration(summary.averageLatencyMs)} hint={t('usage.stat.p95', { value: formatDuration(summary.p95LatencyMs) })} />
      </div>
      <div className={styles.chartGrid}>
        <UsageTrend points={data.timeseries} metric={metric} onMetricChange={onMetricChange} />
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
        <Card title={t('usage.distribution.title')} subtitle={t('usage.distribution.subtitle')} data-od-id="model-distribution"><TokenDistribution rows={data.logicalModels} total={summary.tokens.total} limit={6} /></Card>
      </div>
    </div>
  );
}
