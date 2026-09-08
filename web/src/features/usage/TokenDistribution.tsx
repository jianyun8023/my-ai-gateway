import { Progress } from '@mantine/core';
import type { UsageBreakdownItem } from '@/gateway-usage';
import { EmptyState } from '@/components/ui/EmptyState';
import { formatCompactWithTitle, formatPercent } from '@/utils/formatCompact';
import { useTranslation } from 'react-i18next';
import styles from './Usage.module.scss';

export function TokenDistribution({ rows, total, limit = 8, unreported = false }: { rows: UsageBreakdownItem[]; total: number; limit?: number; unreported?: boolean }) {
  const { t } = useTranslation('console');
  const sorted = [...rows].sort((a, b) => b.tokens.total - a.tokens.total);
  if (!rows.length) return <EmptyState title={t('usage.breakdown.empty')} />;
  return <div className={styles.distributionList}>
    {sorted.slice(0, limit).map(row => {
      const tokens = unreported ? { display: '—', exact: t('usage.composition.unreported') } : formatCompactWithTitle(row.tokens.total);
      const requests = formatCompactWithTitle(row.logicalRequests);
      const share = !unreported && total > 0 ? row.tokens.total / total * 100 : undefined;
      return <div key={row.key}>
        <div className={styles.distributionHeading}><strong>{row.label}</strong><span title={`${tokens.exact} Token`}>{tokens.display} Token · {share === undefined ? '—' : formatPercent(share / 100)}</span></div>
        <Progress value={Math.min(100, share ?? 0)} color="var(--accent)" size={6} aria-label={row.label} aria-valuetext={`${tokens.exact} Token`} />
        <small title={requests.exact}>{t('usage.distribution.requests', { count: requests.display })}</small>
      </div>;
    })}
    {rows.length > limit && <small>{t('usage.distribution.top', { count: limit, total: rows.length })}</small>}
  </div>;
}
