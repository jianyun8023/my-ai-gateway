import { useTranslation } from 'react-i18next';
import { LoadingSpinner } from './LoadingSpinner';
import styles from './ConsolePrimitives.module.scss';

export function LoadingState({ label }: { label?: string }) {
  const { t } = useTranslation('console');
  const text = label ?? t('common.loading');
  return (
    <div className={styles.loadingState} role="status" aria-live="polite" aria-busy="true">
      <LoadingSpinner size={22} />
      <span>{text}</span>
    </div>
  );
}
