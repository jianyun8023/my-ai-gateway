export const GATEWAY_USAGE_TABS = ['overview', 'analysis', 'events'] as const;
export const GATEWAY_MANAGEMENT_PAGES = ['sources', 'model-discovery', 'capabilities', 'models-routes', 'settings'] as const;

export type GatewayUsageTab = typeof GATEWAY_USAGE_TABS[number];
export type GatewayManagementPage = typeof GATEWAY_MANAGEMENT_PAGES[number];
export type GatewayConsoleSpace = 'usage' | 'management';

export type GatewayConsoleRoute =
  | { space: 'usage'; page: GatewayUsageTab }
  | { space: 'management'; page: GatewayManagementPage };

export const DEFAULT_CONSOLE_ROUTES: Record<GatewayConsoleSpace, GatewayConsoleRoute> = {
  usage: { space: 'usage', page: 'overview' },
  management: { space: 'management', page: 'sources' },
};

const normalizeHash = (hash: string): string => hash
  .trim()
  .replace(/^#/, '')
  .replace(/^\/+/, '')
  .replace(/\/+$/, '');

export const resolveConsoleRoute = (hash: string): GatewayConsoleRoute => {
  const normalized = normalizeHash(hash);
  if (GATEWAY_USAGE_TABS.includes(normalized as GatewayUsageTab)) {
    return { space: 'usage', page: normalized as GatewayUsageTab };
  }

  const [space, page, ...rest] = normalized.split('/');
  if (
    rest.length === 0
    && space === 'management'
    && GATEWAY_MANAGEMENT_PAGES.includes(page as GatewayManagementPage)
  ) {
    return { space: 'management', page: page as GatewayManagementPage };
  }

  return DEFAULT_CONSOLE_ROUTES.usage;
};

export const consoleRouteHash = (route: GatewayConsoleRoute): string => route.space === 'usage'
  ? `#${route.page}`
  : `#management/${route.page}`;

export const consoleRoutesEqual = (left: GatewayConsoleRoute, right: GatewayConsoleRoute): boolean => (
  left.space === right.space && left.page === right.page
);
