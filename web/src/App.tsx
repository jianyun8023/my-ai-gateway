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
  IconRoute,
} from './components/ui/icons';
import {
  canonicalConsoleHash,
  CONSOLE_PAGE_DEFINITIONS,
  CONSOLE_SECTIONS,
  consolePageHash,
  isUsagePage,
  resolveConsoleRoute,
  sourceRouteHash,
  type ConsoleNavSection,
  type ConsoleIcon,
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

const NAV_ICONS: Record<ConsoleIcon, React.ReactNode> = {
  dashboard: <IconDashboardGrid size={18} />,
  chart: <IconBarChart size={18} />,
  file: <IconFileText size={18} />,
  database: <IconDatabase size={18} />,
  layers: <IconLayers size={18} />,
  route: <IconRoute size={18} />,
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
  const navigationSections: readonly ConsoleNavSection[] = CONSOLE_SECTIONS.map((section) => ({
    label: t(`shell.section.${section}`),
    pages: CONSOLE_PAGE_DEFINITIONS.filter((entry) => entry.section === section).map(({ id }) => id),
  }));
  const navigationItems: readonly GatewayConsoleNavItem[] = CONSOLE_PAGE_DEFINITIONS.map(({ id, icon }) => ({
    id, label: t(`shell.nav.${id}`), icon: NAV_ICONS[icon],
  }));

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
          focusKey={canonicalConsoleHash(window.location.hash)}
          refreshable
        >
          {({ getAdminKey, adminKeyConfigured, authGeneration, clearAdminKey, refreshRevision, setRefreshing }) => (
            <Suspense fallback={<LoadingState label={t('shell.page_loading')} />}>
              {isUsagePage(page)
                ? (
                    <GatewayUsagePage
                      activeTab={page}
                      requestId={route.requestId}
                      getAdminKey={getAdminKey}
                      authGeneration={authGeneration}
                      refreshRevision={refreshRevision}
                      onLoadingChange={setRefreshing}
                    />
                  )
                : (
                    <GatewayManagementPage
                      key={authGeneration}
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
