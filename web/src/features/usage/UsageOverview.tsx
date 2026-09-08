import { UsageTrend } from './UsageTrend';
import { TokenDistribution } from './TokenDistribution';
import { Card } from '@/components/ui/Card';
import { EmptyState } from '@/components/ui/EmptyState';
import { TokenComposition } from '@/features/usage/TokenComposition';
import styles from '@/features/usage/Usage.module.scss';
import { formatDuration, formatTime, formatUsageTokens } from '@/features/usage/formatters';
import { type TrendMetric } from '@/features/usage/model';
import { type UsageOverviewViewModel } from '@/gateway-usage';
import { MetricCard } from '@/components/ui/MetricCard';
import { UsageBadge } from './UsageBadge';
import { UsageStatus } from './UsageStatus';
import { hasOnlyUnreportedUsage } from './usageQuality';
import { formatCompact, formatExactInteger, formatPercent } from '@/utils/formatCompact';
import { useTranslation } from 'react-i18next';

export function Overview({ data, metric, onMetricChange }: { data: UsageOverviewViewModel; metric: TrendMetric; onMetricChange: (m: TrendMetric) => void }) {
  const { t } = useTranslation('console');
  const { summary } = data;
  const unreported = hasOnlyUnreportedUsage(summary);
  const hasData = summary.logicalRequests > 0 || summary.tokens.total > 0;
  if (!hasData) {
    return <EmptyState title={t('usage.empty.overview_title')} description={t('usage.empty.overview_desc')} />;
  }
  return (
    <div className={styles.stack}>
      <div className={styles.statsGrid} data-od-id="kpi-row">
        <MetricCard label={t('usage.stat.logical_requests')} value={formatCompact(summary.logicalRequests)} exact={formatExactInteger(summary.logicalRequests)} hint={t('usage.stat.upstream_attempts', { count: formatCompact(summary.upstreamAttempts), retries: formatCompact(summary.retries) })} />
        <MetricCard label={t('usage.stat.success_rate')} value={formatPercent(summary.successRate)} hint={t('usage.stat.failures', { count: formatCompact(summary.failedRequests) })} tone={summary.failedRequests > 0 ? 'warning' : 'success'} />
        <MetricCard label={t('usage.stat.total_tokens')} value={unreported ? '—' : formatCompact(summary.tokens.total)} exact={unreported ? undefined : formatExactInteger(summary.tokens.total)} hint={t(unreported ? 'usage.composition.unreported' : 'usage.stat.final_accounting')} />
        <MetricCard label={t('usage.stat.avg_latency')} value={formatDuration(summary.averageLatencyMs)} exact={formatDuration(summary.averageLatencyMs, true)} hint={t('usage.stat.p95', { value: formatDuration(summary.p95LatencyMs) })} />
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
                <UsageStatus success={event.success} />
                <time>{formatTime(event.createdAt)}</time>
                <strong>{event.logicalModel}</strong>
                <span className={styles.recentAttribution}>{event.provider} · {event.sourceId} / {event.account}</span>
                <div className={styles.recentTokens}><em title={formatUsageTokens(event.tokens.total, event.usageSource, true)}>{t('usage.value.tokens', { count: formatUsageTokens(event.tokens.total, event.usageSource) })}</em><UsageBadge source={event.usageSource} /></div>
              </div>
            ))}</div>
          )}
        </Card>
        <Card title={t('usage.distribution.title')} subtitle={t('usage.distribution.subtitle')} data-od-id="model-distribution"><TokenDistribution unreported={unreported} rows={data.logicalModels} total={summary.tokens.total} limit={6} /></Card>
      </div>
    </div>
  );
}
