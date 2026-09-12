import { GatewayAdminResources } from '@/admin-api';
import { AdminClient } from '@/admin-api/client';
import { ModelReviewPage } from '@/features/control-plane/sources/ModelReviewPage';
import { ModelsRoutesPage } from '@/features/control-plane/ModelsRoutesPage';
import { SettingsPage } from '@/features/control-plane/SettingsPage';
import { SourceDetailPage } from '@/features/control-plane/sources/SourceDetailPage';
import { SourceEditPage } from '@/features/control-plane/sources/SourceEditPage';
import { SourcesPage } from '@/features/control-plane/SourcesPage';
import { RuntimeEventsPage } from '@/features/events/RuntimeEventsPage';
import type { ConsoleRoute, GatewayManagementPage as GatewayManagementPageType, SourceSection } from '@/lib/consoleNavigation';
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
  const api = useMemo(() => new GatewayAdminResources(
    new AdminClient({ getAdminKey }),
  ), [getAdminKey]);
  const shared = { api, refreshRevision, onBusyChange: onLoadingChange };

  if (page === 'sources') {
    const sourceRoute = route?.page === 'sources' ? route : undefined;
    const sourceId = sourceRoute?.sourceId;
    if (sourceId && sourceId !== 'new' && sourceRoute?.section === 'review') {
      return <ModelReviewPage {...shared} sourceId={sourceId} onOpenSource={onOpenSource} onNavigatePage={onNavigatePage} />;
    }
    if (sourceId && sourceRoute?.section === 'edit') {
      return (
        <SourceEditPage
          {...shared}
          sourceId={sourceId === 'new' ? undefined : sourceId}
          onOpenSource={onOpenSource}
        />
      );
    }
    if (sourceId) {
      return <SourceDetailPage {...shared} sourceId={sourceId} onOpenSource={onOpenSource} />;
    }
    return <SourcesPage {...shared} onOpenSource={onOpenSource} />;
  }
  if (page === 'runtime-events') return <RuntimeEventsPage {...shared} />;
  if (page === 'models') return <ModelsRoutesPage {...shared} />;
  return (
    <SettingsPage
      {...shared}
      adminKeyConfigured={adminKeyConfigured}
      onClearAdminKey={clearAdminKey}
    />
  );
}
