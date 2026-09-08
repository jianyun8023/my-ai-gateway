import { Progress } from '@mantine/core';
import { Card } from '@/components/ui/Card';
import styles from '@/features/usage/Usage.module.scss';
import { type UsageSummaryViewModel } from '@/gateway-usage';
import { formatCompactWithTitle } from '@/utils/formatCompact';
import { useTranslation } from 'react-i18next';

export function TokenComposition({ summary }: { summary: UsageSummaryViewModel }) {
  const { t } = useTranslation('console');
  const { tokens } = summary;
  const combined = tokens.input + tokens.output;
  const parts = [
    { color: 'var(--accent)', key: 'input', value: tokens.input },
    { color: 'var(--muted)', key: 'output', value: tokens.output },
  ].map(part => ({ ...part, label: t(`usage.legend.${part.key}`), formatted: formatCompactWithTitle(part.value), share: combined > 0 ? part.value / combined * 100 : 0 }));
  const details = [
    { key: 'reasoning', value: tokens.reasoning },
    { key: 'cache_read', value: tokens.cacheRead },
    { key: 'cache_creation', value: tokens.cacheCreation },
  ];
  const total = formatCompactWithTitle(tokens.total);
  const chartLabel = combined > 0
    ? parts.map(part => `${part.label} ${part.formatted.exact} Token (${part.share.toFixed(1)}%)`).join(' · ')
    : t('usage.composition.empty');

  return <Card title={t('usage.composition.title')}>
    <div className={styles.tokenComposition}>
      <div className={styles.tokenTotal}><span>{t('usage.legend.total')}</span><strong title={total.exact}>{total.display}</strong></div>
      <div className={styles.compositionChart}>
        <small className={styles.cacheNote}>{t('usage.composition.chart_title')}</small>
        <Progress.Root size={20} radius="sm" transitionDuration={0} role="img" aria-label={chartLabel}>
          {parts.map(part => <Progress.Section key={part.key} value={part.share} color={part.color} withAria={false} aria-hidden="true" />)}
        </Progress.Root>
        <dl className={styles.compositionLegend}>{parts.map(part => <div key={part.key}>
          <dt><i style={{ background: part.color }} aria-hidden="true" />{part.label}</dt>
          <dd><strong title={part.formatted.exact}>{part.formatted.display}</strong><span>{combined > 0 ? `${part.share.toFixed(1)}%` : '—'}</span></dd>
        </div>)}</dl>
        {combined === 0 && <small className={styles.cacheNote}>{t('usage.composition.empty')}</small>}
        {combined !== tokens.total && <small className={styles.cacheNote}>{t('usage.composition.total_difference', { combined: formatCompactWithTitle(combined).exact, total: total.exact })}</small>}
      </div>
      <dl className={styles.compositionDetails}>{details.map(detail => {
        const formatted = formatCompactWithTitle(detail.value);
        return <div key={detail.key}><dt>{t(`usage.legend.${detail.key}`)}</dt><dd title={formatted.exact}>{formatted.display}</dd></div>;
      })}</dl>
      <small className={styles.cacheNote}>{t('usage.composition.overlap')}</small>
      {(summary.usageSources.missing ?? 0) > 0 && <small className={styles.cacheNote}>{t('usage.composition.missing', { count: summary.usageSources.missing })}</small>}
      {tokens.cacheRead > 0 && <small className={styles.cacheNote}>{t('usage.composition.cache_rate', { rate: tokens.input > 0 ? `${(tokens.cacheRead / tokens.input * 100).toFixed(1)}%` : '—' })}</small>}
    </div>
  </Card>;
}
