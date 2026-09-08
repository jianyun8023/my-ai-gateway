import { currentIntlLocale } from '@/i18n/intl';

export const formatDateTime = (value?: string | null): string => {
  if (!value) return '—';
  const parsed = new Date(value);
  if (Number.isNaN(parsed.getTime())) return value;
  return new Intl.DateTimeFormat(currentIntlLocale(), {
    dateStyle: 'medium',
    timeStyle: 'medium',
  }).format(parsed);
};

export const formatJsonValue = (value: unknown): string => {
  if (value === undefined || value === null) return '—';
  if (typeof value === 'string') return value;
  return JSON.stringify(value);
};
