import { useMemo } from 'react';
import { GatewayAdminResources } from '@/admin-api';
import { CapabilitiesPage } from '@/features/control-plane/CapabilitiesPage';
import { ModelDiscoveryPage } from '@/features/control-plane/ModelDiscoveryPage';
import { ModelsRoutesPage } from '@/features/control-plane/ModelsRoutesPage';
import { SettingsPage } from '@/features/control-plane/SettingsPage';
import { SourcesPage } from '@/features/control-plane/SourcesPage';
import { ControlPlaneClient } from '@/control-plane/client';
import type { GatewayManagementPage as GatewayManagementPageType } from '@/lib/consoleNavigation';

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
    new ControlPlaneClient({ getAdminKey }),
  ), [getAdminKey]);
  const shared = { api, refreshRevision, onBusyChange: onLoadingChange };

  if (page === 'sources') return <SourcesPage {...shared} />;
  if (page === 'model-discovery') return <ModelDiscoveryPage {...shared} />;
  if (page === 'capabilities') return <CapabilitiesPage {...shared} />;
  if (page === 'models-routes') return <ModelsRoutesPage {...shared} />;
  return (
    <SettingsPage
      {...shared}
      adminKeyConfigured={adminKeyConfigured}
      onClearAdminKey={clearAdminKey}
    />
  );
}
