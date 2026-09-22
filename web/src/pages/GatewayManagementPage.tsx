import { GatewayAdminResources } from '@/admin-api';
import { AdminClient } from '@/admin-api/client';
import { UpstreamQuotaPage } from '@/features/quota/UpstreamQuotaPage';
import { ModelReviewPage } from '@/features/control-plane/sources/ModelReviewPage';
import { ModelsRoutesPage } from '@/features/control-plane/ModelsRoutesPage';
import { SettingsPage } from '@/features/control-plane/SettingsPage';
import { SourceDetailPage } from '@/features/control-plane/sources/SourceDetailPage';
import { SourceEditPage } from '@/features/control-plane/sources/SourceEditPage';
import { SourcesPage } from '@/features/control-plane/SourcesPage';
import { RuntimeEventsPage } from '@/features/events/RuntimeEventsPage';
import type { ConsoleRoute, GatewayManagementPage as GatewayManagementPageType, SourceSection } from '@/lib/consoleNavigation';
import { UpstreamQuotaClient } from '@/upstream-quota/client';
import { useMemo } from 'react';

interface GatewayManagementPageProps {
  page: GatewayManagementPageType;
  route?: ConsoleRoute;
  onOpenSource?: (sourceId: string, section?: SourceSection) => void;
  onNavigatePage?: (page: 'models') => void;
  getAdminKey: () => string;
  adminKeyConfigured: boolean;
  clearAdminKey: () => void;
  refreshRevision: number;
  onLoadingChange: (loading: boolean) => void;
}

export function GatewayManagementPage({
  page,
  route,
  onOpenSource,
  onNavigatePage,
  getAdminKey,
  adminKeyConfigured,
  clearAdminKey,
  refreshRevision,
  onLoadingChange,
}: GatewayManagementPageProps) {
  const transport = useMemo(() => new AdminClient({ getAdminKey }), [getAdminKey]);
  const api = useMemo(() => new GatewayAdminResources(transport), [transport]);
  const quotaClient = useMemo(() => new UpstreamQuotaClient(transport), [transport]);
  const shared = { api, refreshRevision, onBusyChange: onLoadingChange };

  if (page === 'upstream-quotas') {
    const quotaRoute = route?.page === 'upstream-quotas' ? route : undefined;
    return (
      <UpstreamQuotaPage
        client={quotaClient}
        api={api}
        accountId={quotaRoute?.accountId}
        refreshRevision={refreshRevision}
        onBusyChange={onLoadingChange}
      />
    );
  }
  if (page === 'sources') {
    const sourceRoute = route?.page === 'sources' ? route : undefined;
    const sourceId = sourceRoute?.sourceId;
    if (sourceId && sourceId !== 'new' && sourceRoute?.section === 'review') {
      return <ModelReviewPage key={`review:${sourceId}`} {...shared} sourceId={sourceId} onOpenSource={onOpenSource} onNavigatePage={onNavigatePage} />;
    }
    if (sourceId && sourceRoute?.section === 'edit') {
      return (
        <SourceEditPage
          key={`edit:${sourceId}`}
          {...shared}
          sourceId={sourceId === 'new' ? undefined : sourceId}
          onOpenSource={onOpenSource}
        />
      );
    }
    if (sourceId) {
      return <SourceDetailPage key={`detail:${sourceId}`} {...shared} sourceId={sourceId} onOpenSource={onOpenSource} />;
    }
    return <SourcesPage {...shared} onOpenSource={onOpenSource} />;
  }
  if (page === 'runtime-events') return <RuntimeEventsPage key={`runtime-events:${route?.correlationId ?? ''}`} {...shared} initialCorrelationId={route?.correlationId} />;
  if (page === 'models') return <ModelsRoutesPage key={`models:${route?.modelSearch ?? ''}`} {...shared} initialSearch={route?.modelSearch} />;
  return (
    <SettingsPage
      {...shared}
      adminKeyConfigured={adminKeyConfigured}
      onClearAdminKey={clearAdminKey}
    />
  );
}
