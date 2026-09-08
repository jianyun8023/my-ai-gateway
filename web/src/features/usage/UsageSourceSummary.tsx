import { useTranslation } from 'react-i18next';
import type { UsageSummaryViewModel } from '@/gateway-usage';
import { formatExactInteger } from '@/utils/formatCompact';
import { UsageBadge } from './UsageBadge';
import { hasOnlyUnreportedUsage, isUnreportedUsage } from './usageQuality';
import styles from './Usage.module.scss';

export function UsageSourceSummary({ summary }: { summary: UsageSummaryViewModel }) {
  const { t } = useTranslation('console');
  const unknown = Object.entries(summary.usageSources).reduce((sum, [source, count]) => sum + (source !== 'missing' && isUnreportedUsage(source) ? count : 0), 0);
  return <div className={styles.usageSources}>
    <span>{t('usage.field.usage_source')}</span>
    <ul>{Object.entries(summary.usageSources).map(([source, count]) => <li key={source}><UsageBadge source={source} /><span>{t('usage.distribution.requests', { count: formatExactInteger(count) })}</span></li>)}</ul>
    {hasOnlyUnreportedUsage(summary) && <small>{t('usage.composition.unreported')}</small>}
    {(summary.usageSources.missing ?? 0) > 0 && <small>{t('usage.composition.missing', { count: formatExactInteger(summary.usageSources.missing) })}</small>}
    {(summary.usageSources.estimated ?? 0) > 0 && <small>{t('usage.composition.estimated', { count: formatExactInteger(summary.usageSources.estimated) })}</small>}
    {unknown > 0 && <small>{t('usage.composition.unknown', { count: formatExactInteger(unknown) })}</small>}
  </div>;
}
