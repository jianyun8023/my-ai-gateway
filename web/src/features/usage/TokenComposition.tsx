import { Progress } from '@mantine/core';
import { Card } from '@/components/ui/Card';
import styles from '@/features/usage/Usage.module.scss';
import { type UsageSummaryViewModel } from '@/gateway-usage';
import { formatCompactWithTitle } from '@/utils/formatCompact';
import { useTranslation } from 'react-i18next';

export function TokenComposition({ summary }: { summary: UsageSummaryViewModel }) {
  const { t } = useTranslation('console');
  const total = summary.tokens.total;
  const rows = [
    { color: 'var(--accent)', key: 'input', label: t('usage.legend.input'), value: summary.tokens.input },
    { color: 'var(--muted)', key: 'output', label: t('usage.legend.output'), value: summary.tokens.output },
    { color: 'var(--warn)', key: 'reasoning', label: t('usage.legend.reasoning'), value: summary.tokens.reasoning },
    { color: 'var(--accent)', key: 'cache_read', label: t('usage.legend.cache_read'), value: summary.tokens.cacheRead },
    { color: 'var(--muted)', key: 'cache_creation', label: t('usage.legend.cache_creation'), value: summary.tokens.cacheCreation },
  ];
  const totalFormatted = formatCompactWithTitle(total);

  return (
    <Card title={t('usage.composition.title')}>
      <div className={styles.tokenComposition}>
        <div className={styles.tokenTotal}>
          <span>{t('usage.legend.total')}</span>
          <strong title={totalFormatted.exact}>{totalFormatted.display}</strong>
        </div>
        <div className={styles.tokenBreakdown}>
          {rows.map((row) => {
            const formatted = formatCompactWithTitle(row.value);
            return (
              <div key={row.key} className={styles.tokenRow}>
                <span>{row.label}</span>
                <Progress className={styles.tokenProgress} value={total > 0 ? Math.min(100, row.value / total * 100) : 0} color={row.color} size={8} aria-label={row.label} aria-valuetext={formatted.exact} />
                <strong title={formatted.exact}>{formatted.display}</strong>
                <small>{total > 0 ? `${(row.value / total * 100).toFixed(1)}%` : '—'}</small>
              </div>
            );
          })}
        </div>
        <small className={styles.cacheNote}>{t('usage.composition.overlap')}</small>
        {(summary.usageSources.missing ?? 0) > 0 && <small className={styles.cacheNote}>{t('usage.composition.missing', { count: summary.usageSources.missing })}</small>}
        {summary.tokens.cacheRead > 0 && (
          <small className={styles.cacheNote}>
            {t('usage.composition.cache_rate', { rate: summary.tokens.input > 0 ? `${(summary.tokens.cacheRead / summary.tokens.input * 100).toFixed(1)}%` : '—' })}
          </small>
        )}
      </div>
    </Card>
  );
}
