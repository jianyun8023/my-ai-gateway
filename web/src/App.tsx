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
  IconDatabase,
  IconFileText,
  IconLayers,
  IconSettings,
  IconSunAsterisk,
} from './components/ui/icons';
import {
  canonicalConsoleHash,
  consolePageHash,
  isUsagePage,
  resolveConsoleRoute,
  sourceRouteHash,
  type ConsoleNavSection,
  type ConsolePage,
  type ConsoleRoute,
  type SourceSection,
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
  'runtime-events': <IconDatabase size={18} />,
  sources: <IconLayers size={18} />,
  models: <IconSunAsterisk size={18} />,
  settings: <IconSettings size={18} />,
};

function App() {
  const { t, i18n } = useTranslation('console');
  const [route, setRoute] = useState<ConsoleRoute>(() => resolveConsoleRoute(window.location.hash));
  const page = route.page;

  const navigateTo = useCallback((nextPage: ConsolePage) => {
    const nextHash = consolePageHash(nextPage);
    if (window.location.hash !== nextHash) window.location.hash = nextHash;
    setRoute({ page: nextPage });
  }, []);

  const navigateToSource = useCallback((sourceId: string, section?: SourceSection) => {
    const nextHash = sourceRouteHash(sourceId, section);
    if (window.location.hash !== nextHash) window.location.hash = nextHash;
    setRoute({ page: 'sources', sourceId, section });
  }, []);

  useEffect(() => {
    const syncRoute = () => {
      const nextRoute = resolveConsoleRoute(window.location.hash);
      const canonicalHash = canonicalConsoleHash(window.location.hash);
      if (window.location.hash !== canonicalHash) {
        window.history.replaceState(null, '', canonicalHash);
      }
      setRoute(nextRoute);
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
    { label: t('shell.section.monitor'), pages: ['overview', 'analysis', 'events', 'runtime-events'] },
    { label: t('shell.section.config'), pages: ['sources', 'models'] },
    { label: t('shell.section.system'), pages: ['settings'] },
  ];

  const navigationItems: readonly GatewayConsoleNavItem[] = [
    { id: 'overview', label: t('shell.nav.overview'), icon: PAGE_ICONS.overview },
    { id: 'analysis', label: t('shell.nav.analysis'), icon: PAGE_ICONS.analysis },
    { id: 'events', label: t('shell.nav.events'), icon: PAGE_ICONS.events },
    { id: 'runtime-events', label: t('shell.nav.runtime-events'), icon: PAGE_ICONS['runtime-events'] },
    { id: 'sources', label: t('shell.nav.sources'), icon: PAGE_ICONS.sources },
    { id: 'models', label: t('shell.nav.models'), icon: PAGE_ICONS.models },
    { id: 'settings', label: t('shell.nav.settings'), icon: PAGE_ICONS.settings },
  ];

  const pageTitle = route.page === 'sources' && route.sourceId
    ? t(route.sourceId === 'new'
      ? 'shell.title.source_new'
      : route.section === 'edit'
        ? 'shell.title.source_edit'
        : route.section === 'review'
          ? 'shell.title.source_review'
          : 'shell.title.source_detail')
    : t(`shell.nav.${page}`);

  useEffect(() => {
    document.documentElement.lang = i18n.language === 'zh' ? 'zh' : 'en';
    document.title = `${pageTitle} · ${t('shell.brand_name')}`;
  }, [i18n.language, pageTitle, t]);

  return (
    <div className="app-frame">
      <main className="app-main">
        <GatewayConsoleShell
          activePage={page}
          navigationSections={navigationSections}
          navigationItems={navigationItems}
          onNavigate={navigate}
          title={pageTitle}
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
                      route={route}
                      onOpenSource={navigateToSource}
                      onNavigatePage={navigateTo}
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
