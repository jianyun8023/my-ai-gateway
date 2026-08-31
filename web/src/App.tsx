import { useCallback, useEffect, useRef, useState } from 'react';
import './index.css';
import './App.css';
import {
  GatewayConsoleShell,
  type GatewayConsoleNavItem,
} from './components/gateway/GatewayConsoleShell';
import {
  IconChartLine,
  IconFileText,
  IconFilterAll,
  IconSearch,
  IconShield,
  IconSidebarProviders,
} from './components/ui/icons';
import {
  consoleRouteHash,
  DEFAULT_CONSOLE_ROUTES,
  GATEWAY_MANAGEMENT_PAGES,
  GATEWAY_USAGE_TABS,
  resolveConsoleRoute,
  type GatewayConsoleRoute,
  type GatewayConsoleSpace,
  type GatewayManagementPage as GatewayManagementPageId,
  type GatewayUsageTab,
} from './lib/consoleNavigation';
import { GatewayManagementPage } from './pages/GatewayManagementPage';
import { GatewayUsagePage } from './pages/GatewayUsagePage';

interface PageMeta {
  title: string;
  shortTitle: string;
  eyebrow: string;
  description: string;
}

const USAGE_META: Record<GatewayUsageTab, PageMeta> = {
  overview: { title: 'Overview', shortTitle: '总览', eyebrow: 'PostgreSQL Usage Events', description: 'Token 总览、时间趋势与最近请求活动' },
  analysis: { title: 'Analysis', shortTitle: '用量分析', eyebrow: 'PostgreSQL Usage Events', description: '模型、Provider、Source、协议与延迟分布' },
  events: { title: 'Request Events', shortTitle: '请求事件', eyebrow: 'PostgreSQL Usage Events', description: '请求元数据、fallback、attempt 与 Token 明细' },
};

const MANAGEMENT_META: Record<GatewayManagementPageId, PageMeta> = {
  sources: { title: 'Sources', shortTitle: '来源管理', eyebrow: 'Control Plane', description: 'Source 与 Account 管理空间' },
  'model-discovery': { title: 'Model Discovery', shortTitle: '模型发现', eyebrow: 'Control Plane', description: '模型发现、差异与确认空间' },
  capabilities: { title: 'Effective Capabilities', shortTitle: '有效能力', eyebrow: 'Control Plane', description: 'DB-first 三协议有效能力空间' },
};

const USAGE_NAVIGATION: readonly GatewayConsoleNavItem[] = [
  { id: 'overview', label: 'Overview', shortLabel: '总览', icon: <IconFilterAll size={18} /> },
  { id: 'analysis', label: 'Analysis', shortLabel: '用量分析', icon: <IconChartLine size={18} /> },
  { id: 'events', label: 'Request Events', shortLabel: '请求事件', icon: <IconFileText size={18} /> },
];

const MANAGEMENT_NAVIGATION: readonly GatewayConsoleNavItem[] = [
  { id: 'sources', label: 'Sources', shortLabel: '来源与账号', icon: <IconSidebarProviders size={18} /> },
  { id: 'model-discovery', label: 'Model Discovery', shortLabel: '发现与确认', icon: <IconSearch size={18} /> },
  { id: 'capabilities', label: 'Effective Capabilities', shortLabel: '三协议矩阵', icon: <IconShield size={18} /> },
];

function App() {
  const [route, setRoute] = useState<GatewayConsoleRoute>(() => resolveConsoleRoute(window.location.hash));
  const lastRouteRef = useRef({
    usage: DEFAULT_CONSOLE_ROUTES.usage,
    management: DEFAULT_CONSOLE_ROUTES.management,
  });

  const rememberRoute = useCallback((nextRoute: GatewayConsoleRoute) => {
    if (nextRoute.space === 'usage') lastRouteRef.current.usage = nextRoute;
    else lastRouteRef.current.management = nextRoute;
  }, []);

  const navigateTo = useCallback((nextRoute: GatewayConsoleRoute) => {
    rememberRoute(nextRoute);
    const nextHash = consoleRouteHash(nextRoute);
    if (window.location.hash !== nextHash) window.location.hash = nextHash;
    setRoute(nextRoute);
  }, [rememberRoute]);

  useEffect(() => {
    const syncRoute = () => {
      const nextRoute = resolveConsoleRoute(window.location.hash);
      const canonicalHash = consoleRouteHash(nextRoute);
      if (window.location.hash !== canonicalHash) {
        window.history.replaceState(null, '', canonicalHash);
      }
      rememberRoute(nextRoute);
      setRoute(nextRoute);
    };
    syncRoute();
    window.addEventListener('hashchange', syncRoute);
    return () => window.removeEventListener('hashchange', syncRoute);
  }, [rememberRoute]);

  const changeSpace = (space: GatewayConsoleSpace) => navigateTo(lastRouteRef.current[space]);
  const navigate = (page: string) => {
    if (route.space === 'usage' && GATEWAY_USAGE_TABS.includes(page as GatewayUsageTab)) {
      navigateTo({ space: 'usage', page: page as GatewayUsageTab });
    } else if (route.space === 'management' && GATEWAY_MANAGEMENT_PAGES.includes(page as GatewayManagementPageId)) {
      navigateTo({ space: 'management', page: page as GatewayManagementPageId });
    }
  };

  const meta = route.space === 'usage' ? USAGE_META[route.page] : MANAGEMENT_META[route.page];

  return (
    <div className="app-frame">
      <main className="app-main">
        <GatewayConsoleShell
          space={route.space}
          navigationLabel={route.space === 'usage' ? '主导航' : '管理导航'}
          navigationSection={route.space === 'usage' ? '监控' : '管理'}
          navigationItems={route.space === 'usage' ? USAGE_NAVIGATION : MANAGEMENT_NAVIGATION}
          activeItem={route.page}
          onNavigate={navigate}
          onSpaceChange={changeSpace}
          title={meta.title}
          shortTitle={meta.shortTitle}
          eyebrow={meta.eyebrow}
          description={meta.description}
          refreshable={route.space === 'usage'}
        >
          {({ getAdminKey, refreshRevision, setRefreshing }) => route.space === 'usage'
            ? (
                <GatewayUsagePage
                  activeTab={route.page}
                  getAdminKey={getAdminKey}
                  refreshRevision={refreshRevision}
                  onLoadingChange={setRefreshing}
                />
              )
            : <GatewayManagementPage page={route.page} />}
        </GatewayConsoleShell>
      </main>
    </div>
  );
}

export default App;
