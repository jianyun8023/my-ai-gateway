import { Card } from '@/components/ui/Card';
import styles from '@/features/usage/Usage.module.scss';
import { type UsageSummaryViewModel } from '@/gateway-usage';
import { formatCompactWithTitle } from '@/utils/formatCompact';
import { useTranslation } from 'react-i18next';

export function TokenComposition({ summary }: { summary: UsageSummaryViewModel }) {
  const { t } = useTranslation('console');
  const total = summary.tokens.total;
  const rows = [
    { key: 'input', label: t('usage.legend.input'), value: summary.tokens.input },
    { key: 'output', label: t('usage.legend.output'), value: summary.tokens.output },
    { key: 'reasoning', label: t('usage.legend.reasoning'), value: summary.tokens.reasoning },
    { key: 'cache_read', label: t('usage.legend.cache_read'), value: summary.tokens.cacheRead },
    { key: 'cache_creation', label: t('usage.legend.cache_creation'), value: summary.tokens.cacheCreation },
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
          {rows.filter((row) => row.value > 0).map((row) => {
            const formatted = formatCompactWithTitle(row.value);
            return (
              <div key={row.key} className={styles.tokenRow}>
                <span>{row.label}</span>
                <div className={styles.tokenBar}>
                  <div style={{ width: `${Math.max(2, (row.value / Math.max(total, 1)) * 100)}%` }} />
                </div>
                <strong title={formatted.exact}>{formatted.display}</strong>
                <small>{((row.value / Math.max(total, 1)) * 100).toFixed(1)}%</small>
              </div>
            );
          })}
        </div>
        {summary.tokens.cacheRead > 0 && (
          <small className={styles.cacheNote}>
            {t('usage.composition.cache_rate', { rate: ((summary.tokens.cacheRead / Math.max(summary.tokens.input, 1)) * 100).toFixed(1) })}
          </small>
        )}
      </div>
    </Card>
  );
}
