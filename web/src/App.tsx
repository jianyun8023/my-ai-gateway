import { lazy, Suspense, useCallback, useEffect, useState } from 'react';
import './index.css';
import './App.css';
import {
  GatewayConsoleShell,
  type GatewayConsoleNavItem,
} from './components/gateway/GatewayConsoleShell';
import {
  IconBarChart,
  IconDashboardGrid,
  IconFileText,
  IconLayers,
  IconSettings,
  IconSunAsterisk,
} from './components/ui/icons';
import {
  consolePageHash,
  resolveConsolePage,
  type ConsolePage,
  type ConsoleNavSection,
} from './lib/consoleNavigation';

const GatewayManagementPage = lazy(async () => {
  const module = await import('./pages/GatewayManagementPage');
  return { default: module.GatewayManagementPage };
});

const GatewayUsagePage = lazy(async () => {
  const module = await import('./pages/GatewayUsagePage');
  return { default: module.GatewayUsagePage };
});

interface PageMeta {
  title: string;
  description: string;
}

const PAGE_META: Record<ConsolePage, PageMeta> = {
  overview: { title: '总览', description: '过去 24 小时的网关运行状态' },
  analysis: { title: '用量分析', description: 'Token 构成、模型分布、来源分析与延迟诊断' },
  events: { title: '请求事件', description: '查看每次请求的元数据、重试和 Token 明细' },
  sources: { title: '来源管理', description: '管理 Provider Source、Account 和连接配置' },
  models: { title: '模型与路由', description: '逻辑模型映射、来源绑定与协议路由配置' },
  settings: { title: '系统设置', description: '网关入口、Virtual Key 和配置管理' },
};

const PAGE_ICONS: Record<ConsolePage, React.ReactNode> = {
  overview: <IconDashboardGrid size={18} />,
  analysis: <IconBarChart size={18} />,
  events: <IconFileText size={18} />,
  sources: <IconLayers size={18} />,
  models: <IconSunAsterisk size={18} />,
  settings: <IconSettings size={18} />,
};

const NAVIGATION_SECTIONS: readonly ConsoleNavSection[] = [
  { label: '监控', pages: ['overview', 'analysis', 'events'] },
  { label: '配置', pages: ['sources', 'models'] },
  { label: '系统', pages: ['settings'] },
];

const NAVIGATION_ITEMS: readonly GatewayConsoleNavItem[] = [
  { id: 'overview', label: '总览', icon: PAGE_ICONS.overview },
  { id: 'analysis', label: '用量分析', icon: PAGE_ICONS.analysis },
  { id: 'events', label: '请求事件', icon: PAGE_ICONS.events },
  { id: 'sources', label: '来源管理', icon: PAGE_ICONS.sources },
  { id: 'models', label: '模型与路由', icon: PAGE_ICONS.models },
  { id: 'settings', label: '系统设置', icon: PAGE_ICONS.settings },
];

const USAGE_PAGES = new Set<ConsolePage>(['overview', 'analysis', 'events']);

function App() {
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

  const meta = PAGE_META[page];

  return (
    <div className="app-frame">
      <main className="app-main">
        <GatewayConsoleShell
          activePage={page}
          navigationSections={NAVIGATION_SECTIONS}
          navigationItems={NAVIGATION_ITEMS}
          onNavigate={navigate}
          title={meta.title}
          description={meta.description}
          refreshable
        >
          {({ getAdminKey, adminKeyConfigured, clearAdminKey, refreshRevision, setRefreshing }) => (
            <Suspense fallback={<div className="app-route-loading" role="status" aria-busy="true">正在加载页面…</div>}>
              {USAGE_PAGES.has(page)
                ? (
                    <GatewayUsagePage
                      activeTab={page as 'overview' | 'analysis' | 'events'}
                      getAdminKey={getAdminKey}
                      refreshRevision={refreshRevision}
                      onLoadingChange={setRefreshing}
                    />
                  )
                : (
                    <GatewayManagementPage
                      page={page === 'models' ? 'models-routes' : page === 'sources' ? 'sources' : page as 'settings'}
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
