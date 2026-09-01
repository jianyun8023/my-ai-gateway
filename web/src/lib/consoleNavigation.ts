/**
 * Flat console navigation matching the prototype design.
 *
 * All pages are accessed via a single-level hash: #overview, #analysis, etc.
 * The sidebar groups them into sections (监控, 配置, 系统) visually,
 * but routing is flat — no space prefix.
 */

export const CONSOLE_PAGES = [
  'overview', 'analysis', 'events',
  'sources', 'models', 'settings',
] as const;

export type ConsolePage = typeof CONSOLE_PAGES[number];

export type ConsoleSection = '监控' | '配置' | '系统';

export interface ConsoleNavSection {
  label: ConsoleSection;
  pages: readonly ConsolePage[];
}

export const CONSOLE_SECTIONS: readonly ConsoleNavSection[] = [
  { label: '监控', pages: ['overview', 'analysis', 'events'] },
  { label: '配置', pages: ['sources', 'models'] },
  { label: '系统', pages: ['settings'] },
];

export const DEFAULT_PAGE: ConsolePage = 'overview';

const normalizeHash = (hash: string): string => hash
  .trim()
  .replace(/^#/, '')
  .replace(/^\/+/, '')
  .replace(/\/+$/, '');

export const resolveConsolePage = (hash: string): ConsolePage => {
  const normalized = normalizeHash(hash);
  if (CONSOLE_PAGES.includes(normalized as ConsolePage)) {
    return normalized as ConsolePage;
  }
  return DEFAULT_PAGE;
};

export const consolePageHash = (page: ConsolePage): string => `#${page}`;

export const consolePagesEqual = (left: ConsolePage, right: ConsolePage): boolean => left === right;

// --- Legacy type aliases for gradual migration of downstream consumers ---

export type GatewayUsageTab = 'overview' | 'analysis' | 'events';
export type GatewayManagementPage = 'sources' | 'model-discovery' | 'capabilities' | 'models-routes' | 'settings';
export type GatewayConsoleSpace = 'usage' | 'management';
export type GatewayConsoleRoute =
  | { space: 'usage'; page: GatewayUsageTab }
  | { space: 'management'; page: GatewayManagementPage };

export const GATEWAY_USAGE_TABS = ['overview', 'analysis', 'events'] as const;
export const GATEWAY_MANAGEMENT_PAGES = ['sources', 'model-discovery', 'capabilities', 'models-routes', 'settings'] as const;

export const DEFAULT_CONSOLE_ROUTES: Record<GatewayConsoleSpace, GatewayConsoleRoute> = {
  usage: { space: 'usage', page: 'overview' },
  management: { space: 'management', page: 'sources' },
};

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
