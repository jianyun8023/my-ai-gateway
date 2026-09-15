import { IconArrowRight, IconBot, IconCode, IconRefreshCw, IconRoute, IconTerminal } from '@/components/ui/icons';
import type { UsageEventViewModel } from '@/gateway-usage';
import { useTranslation } from 'react-i18next';
import { formatEventTime, formatFallbackReason, formatTime } from './formatters';
import styles from './Usage.module.scss';

export function EventTimeCell({ value }: { value: string }) {
  const { time, date } = formatEventTime(value);
  return <time dateTime={date ? value : undefined} title={date ? formatTime(value) : undefined}>
    {time}<small className={styles.eventDate}>{date}</small>
  </time>;
}

export function ModelCell({ event }: { event: UsageEventViewModel }) {
  const { t } = useTranslation('console');
  const fallbackLabel = event.fallbackReason
    ? `${t('usage.event.fallback_only')}: ${formatFallbackReason(t, event.fallbackReason)}`
    : t('usage.event.fallback_only');
  const title = `${t('usage.field.logical_model')}: ${event.logicalModel}\n${t('usage.field.upstream_model')}: ${event.upstreamModel}`;
  return <span className={styles.modelCell} title={title}>
    <span className={styles.modelPrimary}>
      <strong>{event.logicalModel}</strong>
      {event.fallback && <span className={styles.fallbackMarker} role="img" title={fallbackLabel} aria-label={fallbackLabel}><IconRoute size={13} /></span>}
    </span>
    {event.logicalModel !== event.upstreamModel && <small className={styles.modelUpstream}>
      <IconArrowRight size={12} /><span>{event.upstreamModel}</span>
    </small>}
  </span>;
}

const CLIENTS = {
  claude_code: { name: 'Claude Code', Icon: IconCode },
  codex: { name: 'Codex', Icon: IconTerminal },
  codex_cli: { name: 'Codex CLI', Icon: IconTerminal },
  codex_desktop: { name: 'Codex Desktop', Icon: IconTerminal },
  kimi_code: { name: 'Kimi Code', Icon: IconBot },
  curl: { name: 'curl', Icon: IconTerminal },
};

export function ClientSourceCell({ value }: { value: string }) {
  const key = value.trim().toLowerCase().replace(/[\s-]+/g, '_');
  const client = Object.hasOwn(CLIENTS, key) ? CLIENTS[key as keyof typeof CLIENTS] : undefined;
  const Icon = client?.Icon ?? IconTerminal;
  return <span className={styles.clientBadge} title={value}>
    <Icon size={14} /><span>{client?.name ?? (value || '—')}</span>
  </span>;
}

export function RetriesCell({ event }: { event: UsageEventViewModel }) {
  const { t } = useTranslation('console');
  if (event.retryCount === 0 && !event.fallback) return <span>—</span>;
  const label = event.fallback
    ? event.retryCount > 0 ? t('usage.event.retries_fallback', { count: event.retryCount }) : t('usage.event.fallback_only')
    : String(event.retryCount);
  const title = event.fallback && event.fallbackReason
    ? formatFallbackReason(t, event.fallbackReason)
    : t('usage.event.retry_count', { count: event.retryCount });
  return <span className={styles.retryValue} title={title}>
    <IconRefreshCw size={13} />{label}
  </span>;
}
