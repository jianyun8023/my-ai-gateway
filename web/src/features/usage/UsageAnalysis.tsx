import { hasOnlyUnreportedUsage } from './usageQuality';
import { Table } from '@mantine/core';
import { Card } from '@/components/ui/Card';
import { EmptyState } from '@/components/ui/EmptyState';
import { TableScroll } from '@/components/ui/TableScroll';
import { TokenComposition } from './TokenComposition';
import { TokenDistribution } from './TokenDistribution';
import styles from './Usage.module.scss';
import { ANALYSIS_DIMENSIONS } from './model';
import { formatDuration } from './formatters';
import { type UsageBreakdownDimension, type UsageBreakdownItem, type UsageSummaryViewModel } from '@/gateway-usage';
import { formatExactInteger } from '@/utils/formatCompact';
import { useTranslation } from 'react-i18next';

export function Analysis({ breakdowns, summary }: { breakdowns: Partial<Record<UsageBreakdownDimension, UsageBreakdownItem[]>>; summary: UsageSummaryViewModel }) {
  const { t } = useTranslation('console');
  const allRows = Object.values(breakdowns).flatMap(rows => rows ?? []);
  const latencyRows = [...(breakdowns.source_id ?? [])].sort((a, b) => (b.averageLatencyMs ?? -1) - (a.averageLatencyMs ?? -1));
  if (allRows.length === 0 && summary.tokens.total === 0 && summary.logicalRequests === 0) return <EmptyState title={t('usage.empty.analysis_title')} description={t('usage.empty.analysis_desc')} />;
  return <div className={styles.stack}>
    <TokenComposition summary={summary} />
    <div className={styles.analysisGrid}>
      {ANALYSIS_DIMENSIONS.map(({ dimension, titleKey }) => <Card key={dimension} title={t(titleKey)} subtitle={t('usage.breakdown.subtitle')}>
        <TokenDistribution unreported={hasOnlyUnreportedUsage(summary)} rows={breakdowns[dimension] ?? []} total={summary.tokens.total} />
      </Card>)}
    </div>
    <Card title={t('usage.latency.title')} subtitle={t('usage.latency.subtitle')} variant="flush">
      {!latencyRows.length ? <EmptyState title={t('usage.latency.empty')} /> : <TableScroll label={t('usage.latency.title')}>
        <Table className={styles.usageTable} horizontalSpacing="md" verticalSpacing="sm">
          <Table.Thead><Table.Tr><Table.Th>{t('usage.field.source_id')}</Table.Th><Table.Th>{t('usage.stat.avg_latency')}</Table.Th><Table.Th>{t('usage.latency.p95')}</Table.Th><Table.Th>{t('usage.stat.logical_requests')}</Table.Th></Table.Tr></Table.Thead>
          <Table.Tbody>{latencyRows.map(row => <Table.Tr key={row.key}>
            <Table.Td>{row.label}</Table.Td><Table.Td>{formatDuration(row.averageLatencyMs)}</Table.Td><Table.Td>{formatDuration(row.p95LatencyMs)}</Table.Td>
            <Table.Td>{formatExactInteger(row.logicalRequests)}</Table.Td>
          </Table.Tr>)}</Table.Tbody>
        </Table>
      </TableScroll>}
    </Card>
  </div>;
}
