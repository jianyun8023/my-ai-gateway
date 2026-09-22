import { Button } from '@/components/ui/Button';
import { LoadingState } from '@/components/ui/LoadingState';
import { Notice } from '@/components/ui/Notice';
import { GatewayUsageClient } from '@/gateway-usage/client';
import { useAdminQuery } from '@/hooks/useAdminQuery';
import { useLocalizedApiError } from '@/hooks/useLocalizedApiError';
import { consolePageHash, resolveConsoleRoute } from '@/lib/consoleNavigation';
import { useCallback } from 'react';
import { useTranslation } from 'react-i18next';
import { EventDetails } from './UsageEventDetails';

export function DeepLinkedEventDetails({ requestId, client }: { requestId: string; client: GatewayUsageClient }) {
  const { t } = useTranslation('console');
  const localizeError = useLocalizedApiError();
  const load = useCallback((signal: AbortSignal) => client.eventByRequestId(requestId, signal), [client, requestId]);
  const query = useAdminQuery({ load });
  const close = () => {
    if (resolveConsoleRoute(window.location.hash).requestId === requestId) {
      window.location.hash = consolePageHash('events');
    }
  };

  if (query.data) {
    return <EventDetails event={query.data} client={client} initialAttempts={query.data.attempts} onClose={close} />;
  }
  if (query.error) {
    return <Notice action={<>
      <Button size="sm" variant="secondary" onClick={query.reload}>{t('common.retry')}</Button>
      <Button size="sm" variant="ghost" onClick={close}>{t('common.close')}</Button>
    </>}>{t('usage.detail.request_load_failed', { id: requestId })}: {localizeError(query.error)}</Notice>;
  }
  return <LoadingState label={t('usage.detail.request_loading', { id: requestId })} />;
}
