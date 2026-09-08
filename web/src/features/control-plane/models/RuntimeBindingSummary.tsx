import { StatusPill } from '@/components/ui/StatusPill';
import styles from '@/features/control-plane/ControlPlane.module.scss';
import { type ResolvedBindingCell } from '@/features/control-plane/models/catalog';
import { PROTOCOL_LABELS } from '@/lib/protocols';
import { useTranslation } from 'react-i18next';

export function RuntimeBindingSummary({ cells }: { cells: ResolvedBindingCell[] }) {
  const { t } = useTranslation('console');
  if (cells.length === 0) return <StatusPill tone="muted">{t('models.state.not_published')}</StatusPill>;
  return (
    <span className={styles.runtimeSummary}>
      {cells.map(({ routeId, cell }) => (
        <span key={`${routeId}:${cell.protocol_in}`}>
          <StatusPill tone={cell.mode === 'native' ? 'success' : 'warning'}>{t(`values.mode.${cell.mode}`, { defaultValue: cell.mode })}</StatusPill>
          <small>{PROTOCOL_LABELS[cell.protocol_in]} → {cell.protocol_upstream ? PROTOCOL_LABELS[cell.protocol_upstream] : t('common.unknown')}</small>
          <StatusPill tone={cell.selection === 'primary' ? 'accent' : 'muted'}>{t('models.state.selection', { rank: cell.selection_rank })}</StatusPill>
        </span>
      ))}
    </span>
  );
}
