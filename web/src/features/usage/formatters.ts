import { currentIntlLocale } from '@/i18n/intl';
import { isUnreportedUsage } from './usageQuality';
import { formatCompact, formatExactInteger } from '@/utils/formatCompact';
import type { TFunction } from 'i18next';

const FALLBACK_REASON_KEYS: Record<string, string> = {
  account_disabled: 'usage.fallback_reason.account_disabled',
  account_cooling_down: 'usage.fallback_reason.account_cooling_down',
  account_unhealthy: 'usage.fallback_reason.account_unhealthy',
  account_unavailable: 'usage.fallback_reason.account_unavailable',
  upstream_transport_error: 'usage.fallback_reason.upstream_transport_error',
};

export const formatFallbackReason = (t: TFunction, reason: string): string => {
  const http = /^upstream_http_(\d+)$/.exec(reason);
  if (http) return t('usage.fallback_reason.upstream_http', { code: http[1] });
  const key = FALLBACK_REASON_KEYS[reason];
  return key ? t(key) : t('usage.fallback_reason.other', { reason });
};

export const formatTime = (value: string): string => {
  if (!value) return '—';
  return new Intl.DateTimeFormat(currentIntlLocale(), {
    dateStyle: 'short',
    timeStyle: 'medium',
  }).format(new Date(value));
};

export const formatBucket = (value: string): string => new Intl.DateTimeFormat(currentIntlLocale(), {
  month: 'short',
  day: 'numeric',
  hour: '2-digit',
  minute: '2-digit',
}).format(new Date(value));

export const formatDuration = (value?: number | null, exact = false): string => {
  if (value == null || !Number.isFinite(value) || value < 0) return '—';
  if (exact || value < 1000) return `${formatExactInteger(value)} ms`;
  return `${(value / 1000).toFixed(value < 10_000 ? 1 : 0)} s`;
};

export const formatUsageTokens = (value: number, source: string, exact = false): string => {
  if (value === 0 && isUnreportedUsage(source)) return '—';
  return exact ? formatExactInteger(value) : formatCompact(value);
};
