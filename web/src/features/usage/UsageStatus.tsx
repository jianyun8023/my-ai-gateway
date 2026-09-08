import { StatusPill } from '@/components/ui/StatusPill';
import { useTranslation } from 'react-i18next';

export function UsageStatus({ success, statusCode }: { success: boolean; statusCode?: number }) {
  const { t } = useTranslation('console');
  return <StatusPill tone={success ? 'success' : 'danger'}>{statusCode === undefined ? '' : `${statusCode || '—'} · `}{t(success ? 'usage.event.success' : 'usage.event.failure')}</StatusPill>;
}
