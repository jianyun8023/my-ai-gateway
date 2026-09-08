import { Paper } from '@mantine/core';
import { useTranslation } from 'react-i18next';
import { LoadingSpinner } from './LoadingSpinner';
import styles from './Feedback.module.scss';

export function LoadingState({ label, layout = 'panel' }: { label?: string; layout?: 'panel' | 'inline' }) {
  const { t } = useTranslation('console');
  return <Paper withBorder={layout === 'panel'} className={styles.loadingState} data-layout={layout}
    role="status" aria-live="polite" aria-busy="true"
  >
    <LoadingSpinner size={22} />
    <span className={styles.loadingLabel}>{label ?? t('common.loading')}</span>
  </Paper>;
}
