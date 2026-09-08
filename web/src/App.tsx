import { lazy, Suspense, useCallback, useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import './App.css';
import { LoadingState } from './components/ui/LoadingState';
import {
  GatewayConsoleShell,
  type GatewayConsoleNavItem,
} from './components/gateway/GatewayConsoleShell';
import {
  IconBarChart,
  IconDashboardGrid,
  IconFileText,
  IconLayers,
  IconSearch,
  IconSettings,
  IconSlidersHorizontal,
  IconSunAsterisk,
} from './components/ui/icons';
import {
  consolePageHash,
  isUsagePage,
  resolveConsolePage,
  type ConsoleNavSection,
  type ConsolePage,
} from './lib/consoleNavigation';

const GatewayManagementPage = lazy(async () => {
  const module = await import('./pages/GatewayManagementPage');
  return { default: module.GatewayManagementPage };
});

const GatewayUsagePage = lazy(async () => {
  const module = await import('./pages/GatewayUsagePage');
  return { default: module.GatewayUsagePage };
});

const PAGE_ICONS: Record<ConsolePage, React.ReactNode> = {
  overview: <IconDashboardGrid size={18} />,
  analysis: <IconBarChart size={18} />,
  events: <IconFileText size={18} />,
  sources: <IconLayers size={18} />,
  discovery: <IconSearch size={18} />,
  models: <IconSunAsterisk size={18} />,
  capabilities: <IconSlidersHorizontal size={18} />,
  settings: <IconSettings size={18} />,
};

function App() {
  const { t, i18n } = useTranslation('console');
  const [page, setPage] = useState<ConsolePage>(() => resolveConsolePage(window.location.hash));

  const navigateTo = useCallback((nextPage: ConsolePage) => {
    const nextHash = consolePageHash(nextPage);
    if (window.location.hash !== nextHash) window.location.hash = nextHash;
    setPage(nextPage);
  }, []);

  useEffect(() => {
    const syncRoute = () => {
      const nextPage = resolveConsolePage(window.location.hash);
      const canonicalHash = consolePageHash(nextPage);
      if (window.location.hash !== canonicalHash) {
        window.history.replaceState(null, '', canonicalHash);
      }
      setPage(nextPage);
    };
    syncRoute();
    window.addEventListener('hashchange', syncRoute);
    return () => window.removeEventListener('hashchange', syncRoute);
  }, []);

  const navigate = (id: string) => {
    const target = id as ConsolePage;
    navigateTo(target);
  };

  // Navigation copy is derived from the console namespace so sidebar labels,
  // page headers and <title> follow the active language.
  const navigationSections: readonly ConsoleNavSection[] = [
    { label: t('shell.section.monitor'), pages: ['overview', 'analysis', 'events'] },
    { label: t('shell.section.config'), pages: ['sources', 'discovery', 'models', 'capabilities'] },
    { label: t('shell.section.system'), pages: ['settings'] },
  ];

  const navigationItems: readonly GatewayConsoleNavItem[] = [
    { id: 'overview', label: t('shell.nav.overview'), icon: PAGE_ICONS.overview },
    { id: 'analysis', label: t('shell.nav.analysis'), icon: PAGE_ICONS.analysis },
    { id: 'events', label: t('shell.nav.events'), icon: PAGE_ICONS.events },
    { id: 'sources', label: t('shell.nav.sources'), icon: PAGE_ICONS.sources },
    { id: 'discovery', label: t('shell.nav.discovery'), icon: PAGE_ICONS.discovery },
    { id: 'models', label: t('shell.nav.models'), icon: PAGE_ICONS.models },
    { id: 'capabilities', label: t('shell.nav.capabilities'), icon: PAGE_ICONS.capabilities },
    { id: 'settings', label: t('shell.nav.settings'), icon: PAGE_ICONS.settings },
  ];

  useEffect(() => {
    document.documentElement.lang = i18n.language === 'zh' ? 'zh' : 'en';
    document.title = `${t(`shell.nav.${page}`)} · ${t('shell.brand_name')}`;
  }, [i18n.language, page, t]);

  return (
    <div className="app-frame">
      <main className="app-main">
        <GatewayConsoleShell
          activePage={page}
          navigationSections={navigationSections}
          navigationItems={navigationItems}
          onNavigate={navigate}
          title={t(`shell.nav.${page}`)}
          refreshable
        >
          {({ getAdminKey, adminKeyConfigured, authGeneration, clearAdminKey, refreshRevision, setRefreshing }) => (
            <Suspense fallback={<LoadingState label={t('shell.page_loading')} />}>
              {isUsagePage(page)
                ? (
                    <GatewayUsagePage
                      activeTab={page}
                      getAdminKey={getAdminKey}
                      authGeneration={authGeneration}
                      refreshRevision={refreshRevision}
                      onLoadingChange={setRefreshing}
                    />
                  )
                : (
                    <GatewayManagementPage
                      page={page}
                      getAdminKey={getAdminKey}
                      adminKeyConfigured={adminKeyConfigured}
                      clearAdminKey={clearAdminKey}
                      refreshRevision={refreshRevision}
                      onLoadingChange={setRefreshing}
                    />
                  )}
            </Suspense>
          )}
        </GatewayConsoleShell>
      </main>
    </div>
  );
}

export default App;
