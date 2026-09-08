import { NativeSelect, Table } from '@mantine/core';
import { useState } from 'react';
import { Bar, Line } from 'react-chartjs-2';
import { useTranslation } from 'react-i18next';
import type { UsageTimeseriesPoint } from '@/gateway-usage';
import { Button } from '@/components/ui/Button';
import { Card } from '@/components/ui/Card';
import { EmptyState } from '@/components/ui/EmptyState';
import { TableScroll } from '@/components/ui/TableScroll';
import { formatExactInteger } from '@/utils/formatCompact';
import '@/lib/chartjs';
import { formatBucket } from './formatters';
import { metricTrendOptions, stackedTrendOptions } from './charts';
import { useChartTheme } from './useChartTheme';
import type { TrendMetric } from './model';
import styles from './Usage.module.scss';

const METRICS: TrendMetric[] = ['composition', 'total', 'input', 'output', 'reasoning', 'cached', 'requests'];
function metricValues(points: UsageTimeseriesPoint[], metric: Exclude<TrendMetric, 'composition'>) {
  return points.map(point => metric === 'requests' ? point.logicalRequests : point.tokens[metric]);
}

export function UsageTrend({ points, metric, onMetricChange }: { points: UsageTimeseriesPoint[]; metric: TrendMetric; onMetricChange: (value: TrendMetric) => void }) {
  const { t } = useTranslation('console');
  const theme = useChartTheme();
  const [showData, setShowData] = useState(false);
  const secondary = metric === 'requests' ? 'total' : 'requests';
  const series = metric === 'composition'
    ? [{ label: t('usage.legend.input'), values: metricValues(points, 'input') }, { label: t('usage.legend.output'), values: metricValues(points, 'output') }]
    : [{ label: t(`usage.metric.${metric}`), values: metricValues(points, metric) }, { label: t(`usage.metric.${secondary}`), values: metricValues(points, secondary) }];
  const labels = points.map(point => formatBucket(point.bucket));
  return <Card title={t('usage.trend.title')} subtitle={t('usage.trend.subtitle')} data-od-id="token-trend" extra={
    <NativeSelect aria-label={t('usage.trend.title')} value={metric} onChange={event => onMetricChange(event.target.value as TrendMetric)} className={styles.metricSelect}>
      {METRICS.map(value => <option key={value} value={value}>{t(`usage.metric.${value}`)}</option>)}
    </NativeSelect>
  }>
    {!points.length ? <EmptyState title={t('usage.trend.empty')} /> : <>
      <div className={styles.chartLarge}>
        {theme && (metric === 'composition'
          ? <Bar role="img" aria-label={`${t('usage.trend.title')} · ${t('usage.metric.composition')}`} fallbackContent={t('usage.trend.view_data')}
            data={{ labels, datasets: series.map((item, index) => ({ label: item.label, data: item.values, stack: 'tokens',
              backgroundColor: index ? theme.secondary : theme.accent, hoverBackgroundColor: index ? theme.secondary : theme.accent,
              borderRadius: 3, maxBarThickness: 32 })) }} options={stackedTrendOptions(theme)} />
          : <Line role="img" aria-label={`${t('usage.trend.title')} · ${t(`usage.metric.${metric}`)}`} fallbackContent={t('usage.trend.view_data')}
            data={{ labels, datasets: series.map((item, index) => ({ label: item.label, data: item.values,
              borderColor: index ? theme.secondary : theme.accent, backgroundColor: index ? theme.secondary : theme.accent,
              pointBackgroundColor: index ? theme.secondary : theme.accent, pointHoverBackgroundColor: index ? theme.secondary : theme.accent,
              pointHoverBorderColor: index ? theme.secondary : theme.accent, pointRadius: 2, borderWidth: 2, tension: 0.2,
              fill: false, yAxisID: index ? 'secondary' : 'y' })) }} options={metricTrendOptions(theme, series[0].label, series[1].label)} />)}
      </div>
      <div className={styles.chartFooter}>
        <small>{metric === 'composition' ? t('usage.trend.composition_hint') : t('usage.trend.axes_hint')}</small>
        <Button variant="ghost" size="sm" aria-expanded={showData} onClick={() => setShowData(!showData)}>{t(showData ? 'usage.trend.hide_data' : 'usage.trend.view_data')}</Button>
      </div>
      {showData && <div className={styles.chartData}><TableScroll label={t('usage.trend.view_data')}>
        <Table className={styles.usageTable} horizontalSpacing="sm" verticalSpacing="xs">
          <Table.Thead><Table.Tr><Table.Th>{t('usage.field.time')}</Table.Th>{series.map(item => <Table.Th key={item.label}>{item.label}</Table.Th>)}</Table.Tr></Table.Thead>
          <Table.Tbody>{points.map((point, index) => <Table.Tr key={point.bucket}><Table.Td>{labels[index]}</Table.Td>{series.map(item => <Table.Td key={item.label}>{formatExactInteger(item.values[index])}</Table.Td>)}</Table.Tr>)}</Table.Tbody>
        </Table>
      </TableScroll></div>}
    </>}
  </Card>;
}
