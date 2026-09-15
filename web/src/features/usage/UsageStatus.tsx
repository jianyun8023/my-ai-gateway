import { StatusPill } from '@/components/ui/StatusPill';
import { IconTriangleAlert, IconX } from '@/components/ui/icons';
import styles from './Usage.module.scss';
import { useTranslation } from 'react-i18next';

export function UsageStatus({ success, statusCode }: { success: boolean; statusCode?: number }) {
  const { t } = useTranslation('console');
  const clientError = !success && statusCode !== undefined && statusCode >= 400 && statusCode < 500;
  return <StatusPill tone={success ? 'success' : clientError ? 'warning' : 'danger'}>
    <span className={styles.statusContent}>
      {success ? <span className={styles.statusDot} aria-hidden="true" />
        : clientError ? <IconTriangleAlert size={12} /> : <IconX size={12} />}
      <span>{statusCode === undefined ? '' : `${statusCode || '—'} · `}{t(success ? 'usage.event.success' : 'usage.event.failure')}</span>
    </span>
  </StatusPill>;
}
