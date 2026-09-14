import { StatusPill, type StatusTone } from '@/components/ui/StatusPill';
import { Tooltip } from '@/components/ui/overlays';
import styles from '@/features/usage/Usage.module.scss';
import { useTranslation } from 'react-i18next';

// Badge shows the short label; the tooltip carries the full label plus what the
// value actually means (trusted source vs. capture method). `parsed` is reported
// by the upstream over SSE, so it shares the upstream success tone.
// The wrapper span is the tooltip target: StatusPill does not forward refs.
export function UsageBadge({ source }: { source: string }) {
  const { t } = useTranslation('console');
  const tones: Record<string, StatusTone> = { upstream: 'success', parsed: 'success', estimated: 'warning', missing: 'danger' };
  const full = t(`usage.usage_source.${source}`, { defaultValue: source });
  const desc = t(`usage.usage_source_desc.${source}`, { defaultValue: '' });
  return (
    <Tooltip label={<span><strong>{full}</strong>{desc ? <><br />{desc}</> : null}</span>} events={{ hover: true, focus: true, touch: true }}>
      <span className={styles.badgeTarget}><StatusPill tone={tones[source] ?? 'muted'}>{t(`usage.usage_source_short.${source}`, { defaultValue: source })}</StatusPill></span>
    </Tooltip>
  );
}
