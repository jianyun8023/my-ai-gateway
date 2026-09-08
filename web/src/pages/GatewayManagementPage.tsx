import { GatewayAdminResources } from '@/admin-api';
import { AdminClient } from '@/admin-api/client';
import { CapabilitiesPage } from '@/features/control-plane/CapabilitiesPage';
import { ModelDiscoveryPage } from '@/features/control-plane/ModelDiscoveryPage';
import { ModelsRoutesPage } from '@/features/control-plane/ModelsRoutesPage';
import { SettingsPage } from '@/features/control-plane/SettingsPage';
import { SourcesPage } from '@/features/control-plane/SourcesPage';
import { RuntimeEventsPage } from '@/features/events/RuntimeEventsPage';
import type { GatewayManagementPage as GatewayManagementPageType } from '@/lib/consoleNavigation';
import { useMemo } from 'react';

interface GatewayManagementPageProps {
  page: GatewayManagementPageType;
  getAdminKey: () => string;
  adminKeyConfigured: boolean;
  clearAdminKey: () => void;
  refreshRevision: number;
  onLoadingChange: (loading: boolean) => void;
}

export function GatewayManagementPage({
  page,
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

  if (page === 'sources') return <SourcesPage {...shared} />;
  if (page === 'runtime-events') return <RuntimeEventsPage {...shared} />;
  if (page === 'discovery') return <ModelDiscoveryPage {...shared} />;
  if (page === 'capabilities') return <CapabilitiesPage {...shared} />;
  if (page === 'models') return <ModelsRoutesPage {...shared} />;
  return (
    <SettingsPage
      {...shared}
      adminKeyConfigured={adminKeyConfigured}
      onClearAdminKey={clearAdminKey}
    />
  );
}
