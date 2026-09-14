import { HoverCard } from '@/components/ui/overlays';
import { UsageBadge } from '@/features/usage/UsageBadge';
import { formatUsageTokens } from '@/features/usage/formatters';
import { cacheHitRate, isUnreportedUsage } from '@/features/usage/usageQuality';
import styles from '@/features/usage/Usage.module.scss';
import type { UsageEventViewModel } from '@/gateway-usage';
import { formatPercent } from '@/utils/formatCompact';
import { useTranslation } from 'react-i18next';

// Full token breakdown shown when hovering the compact Token cell. Exact
// integer values; the row itself only carries the compact total.
export function TokenBreakdown({ event }: { event: UsageEventViewModel }) {
  const { t } = useTranslation('console');
  const rows: Array<[string, string]> = [
    [t('usage.legend.input'), formatUsageTokens(event.tokens.input, event.usageSource, true)],
    [t('usage.legend.output'), formatUsageTokens(event.tokens.output, event.usageSource, true)],
    [t('usage.legend.reasoning'), formatUsageTokens(event.tokens.reasoning, event.usageSource, true)],
    [t('usage.legend.cache_read'), formatUsageTokens(event.tokens.cacheRead, event.usageSource, true)],
    [t('usage.legend.cache_creation'), formatUsageTokens(event.tokens.cacheCreation, event.usageSource, true)],
    [t('usage.legend.total'), formatUsageTokens(event.tokens.total, event.usageSource, true)],
  ];
  return (
    <div className={styles.metricPopover} data-od-id="token-breakdown">
      <strong className={styles.metricPopoverTitle}>{t('usage.events.token_details_title')}</strong>
      <dl>{rows.map(([label, value]) => <div key={label}><dt>{label}</dt><dd>{value}</dd></div>)}</dl>
      <div className={styles.metricPopoverSource}>
        <span>{t('usage.field.usage_source')}</span>
        <UsageBadge source={event.usageSource} />
      </div>
    </div>
  );
}

// Cache hit breakdown shown when hovering the compact cache-rate cell.
export function CacheBreakdown({ event }: { event: UsageEventViewModel }) {
  const { t } = useTranslation('console');
  const rate = cacheHitRate(event.tokens);
  const rows: Array<[string, string]> = [
    [t('usage.field.cache_hit_rate'), rate === null ? '—' : formatPercent(rate)],
    [t('usage.legend.cache_read'), formatUsageTokens(event.tokens.cacheRead, event.usageSource, true)],
    [t('usage.legend.cache_creation'), formatUsageTokens(event.tokens.cacheCreation, event.usageSource, true)],
  ];
  return (
    <div className={styles.metricPopover} data-od-id="cache-breakdown">
      <strong className={styles.metricPopoverTitle}>{t('usage.events.cache_details_title')}</strong>
      <dl>{rows.map(([label, value]) => <div key={label}><dt>{label}</dt><dd>{value}</dd></div>)}</dl>
      <p className={styles.metricPopoverNote}>{t('usage.events.cache_basis')}</p>
    </div>
  );
}

export function TokenCell({ event }: { event: UsageEventViewModel }) {
  if (event.tokens.total === 0 && isUnreportedUsage(event.usageSource)) {
    return <UsageBadge source={event.usageSource} />;
  }
  return (
    <HoverCard position="bottom-end" shadow="md" radius={8} withinPortal>
      <HoverCard.Target>
        <span className={styles.metricValue}>{formatUsageTokens(event.tokens.total, event.usageSource)}</span>
      </HoverCard.Target>
      <HoverCard.Dropdown>
        <TokenBreakdown event={event} />
      </HoverCard.Dropdown>
    </HoverCard>
  );
}

export function CacheCell({ event }: { event: UsageEventViewModel }) {
  const rate = cacheHitRate(event.tokens);
  return (
    <HoverCard position="bottom-end" shadow="md" radius={8} withinPortal>
      <HoverCard.Target>
        <span className={styles.metricValue}>{rate === null ? '—' : formatPercent(rate)}</span>
      </HoverCard.Target>
      <HoverCard.Dropdown>
        <CacheBreakdown event={event} />
      </HoverCard.Dropdown>
    </HoverCard>
  );
}
