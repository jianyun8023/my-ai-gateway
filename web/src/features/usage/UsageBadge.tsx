import { StatusPill, type StatusTone } from '@/components/ui/StatusPill';
import { useTranslation } from 'react-i18next';

export function UsageBadge({ source }: { source: string }) {
  const { t } = useTranslation('console');
  const tones: Record<string, StatusTone> = { upstream: 'success', estimated: 'warning', missing: 'danger' };
  return <StatusPill tone={tones[source] ?? 'muted'}>{t(`usage.usage_source.${source}`, { defaultValue: source })}</StatusPill>;
}
