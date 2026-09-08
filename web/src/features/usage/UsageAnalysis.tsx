import { Card } from '@/components/ui/Card';
import { EmptyState } from '@/components/ui/EmptyState';
import { TokenComposition } from '@/features/usage/TokenComposition';
import styles from '@/features/usage/Usage.module.scss';
import { horizontalTokenBarOptions, makeChartColors } from '@/features/usage/charts';
import { ANALYSIS_DIMENSIONS } from '@/features/usage/model';
import { type UsageBreakdownDimension, type UsageBreakdownItem, type UsageSummaryViewModel } from '@/gateway-usage';
import '@/lib/chartjs';
import { formatExactInteger } from '@/utils/formatCompact';
import { Bar } from 'react-chartjs-2';
import { useTranslation } from 'react-i18next';

function BreakdownChart({ title, rows }: { title: string; rows: UsageBreakdownItem[] }) {
  const { t } = useTranslation('console');
  const visibleRows = rows.slice(0, 8);
  return (
    <Card title={title} subtitle={t('usage.breakdown.subtitle')}>
      {visibleRows.length === 0 ? <EmptyState title={t('usage.breakdown.empty')} /> : (
        <div className={styles.breakdownChart}>
          <Bar data={{ labels: visibleRows.map((item) => item.label), datasets: [{ label: t('usage.legend.total'), data: visibleRows.map((item) => item.tokens.total), backgroundColor: makeChartColors()[0], borderRadius: 6 }] }} options={horizontalTokenBarOptions} />
        </div>
      )}
    </Card>
  );
}

export function Analysis({ breakdowns, summary }: { breakdowns: Partial<Record<UsageBreakdownDimension, UsageBreakdownItem[]>>; summary: UsageSummaryViewModel }) {
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
              <Bar data={{ labels: latencyRows.map((item) => item.label), datasets: [{ label: t('usage.latency.dataset'), data: latencyRows.map((item) => item.averageLatencyMs ?? 0), backgroundColor: makeChartColors()[2], borderRadius: 6 }] }} options={{ responsive: true, maintainAspectRatio: false, plugins: { legend: { display: false }, tooltip: { callbacks: { label: (context) => `${context.dataset.label}: ${formatExactInteger(Number(context.parsed.y ?? 0))} ms` } } }, scales: { y: { ticks: { callback: (value) => `${formatExactInteger(Number(value))} ms` } } } }} />
            </div>
          )}
        </Card>
      </div>
    </div>
  );
}
