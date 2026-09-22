export const CONSOLE_SECTIONS = ['monitor', 'config', 'system'] as const;

// Route metadata stays independent of React and UI components. App resolves
// icon names and translation keys when composing the navigation.
export const CONSOLE_PAGE_DEFINITIONS = [
  { id: 'overview', section: 'monitor', space: 'usage', icon: 'dashboard' },
  { id: 'analysis', section: 'monitor', space: 'usage', icon: 'chart' },
  { id: 'events', section: 'monitor', space: 'usage', icon: 'file' },
  { id: 'upstream-quotas', section: 'monitor', space: 'management', icon: 'database' },
  { id: 'runtime-events', section: 'monitor', space: 'management', icon: 'database' },
  { id: 'sources', section: 'config', space: 'management', icon: 'layers' },
  { id: 'models', section: 'config', space: 'management', icon: 'route' },
  { id: 'settings', section: 'system', space: 'management', icon: 'settings' },
] as const;

type ConsolePageDefinition = typeof CONSOLE_PAGE_DEFINITIONS[number];
export type ConsolePage = ConsolePageDefinition['id'];
export type ConsoleIcon = ConsolePageDefinition['icon'];
export const CONSOLE_PAGES = CONSOLE_PAGE_DEFINITIONS.map(({ id }) => id);
export type GatewayUsageTab = Extract<ConsolePageDefinition, { space: 'usage' }>['id'];
export type GatewayManagementPage = Exclude<ConsolePage, GatewayUsageTab>;
export type SourceSection = 'edit' | 'review';
export interface ConsoleNavSection { label: string; pages: readonly ConsolePage[] }

/** 控制台路由；来源与上游额度拥有各自的二级详情地址。 */
export interface ConsoleRoute {
  page: ConsolePage;
  /** page === 'sources' 时存在；'new' 表示新增来源工作流。 */
  sourceId?: string;
  /** sourceId 对应的子页面，缺省为来源详情。 */
  section?: SourceSection;
  /** page === 'upstream-quotas' 时存在，表示账号额度详情。 */
  accountId?: string;
  /** page === 'runtime-events' 时从请求事件带入的关联 ID。 */
  correlationId?: string;
  /** page === 'events' 时定位单条请求详情。 */
  requestId?: string;
  /** page === 'models' 时从请求事件带入的逻辑模型搜索词。 */
  modelSearch?: string;
}

export function isUsagePage(page: ConsolePage): page is GatewayUsageTab {
  return CONSOLE_PAGE_DEFINITIONS.some((entry) => entry.id === page && entry.space === 'usage');
}

const splitHash = (hash: string): { path: string; params: URLSearchParams } => {
  const normalized = hash.trim().replace(/^#/, '');
  const [path, query = ''] = normalized.split('?', 2);
  return { path, params: new URLSearchParams(query) };
};

const normalizeHash = (hash: string): string[] => splitHash(hash).path
  .trim()
  .replace(/^\/+/, '')
  .replace(/\/+$/, '')
  .split('/')
  .filter(Boolean)
  .map((segment) => decodeURIComponent(segment));

export const resolveConsoleRoute = (hash: string): ConsoleRoute => {
  const segments = normalizeHash(hash);
  const { params } = splitHash(hash);
  const head = segments[0] as ConsolePage | undefined;
  if (!head || !CONSOLE_PAGES.includes(head)) return { page: 'overview' };
  if (head === 'sources' && segments[1]) {
    const section = segments[2] === 'edit' || segments[2] === 'review' ? segments[2] : undefined;
    return { page: 'sources', sourceId: segments[1], section };
  }
  if (head === 'upstream-quotas' && segments[1]) {
    return { page: 'upstream-quotas', accountId: segments[1] };
  }
  if (head === 'runtime-events') {
    return { page: head, correlationId: params.get('correlation_id')?.trim() || undefined };
  }
  if (head === 'events') {
    return { page: head, requestId: params.get('request_id')?.trim() || undefined };
  }
  if (head === 'models') {
    return { page: head, modelSearch: params.get('model')?.trim() || undefined };
  }
  return { page: head };
};

export const resolveConsolePage = (hash: string): ConsolePage => resolveConsoleRoute(hash).page;

export const consolePageHash = (page: ConsolePage): string => '#' + page;

export const sourceRouteHash = (sourceId: string, section?: SourceSection): string => (
  `#sources/${encodeURIComponent(sourceId)}${section ? `/${section}` : ''}`
);

export const upstreamQuotaRouteHash = (accountId?: string): string => (
  accountId ? `#upstream-quotas/${encodeURIComponent(accountId)}` : '#upstream-quotas'
);

export const runtimeEventsRouteHash = (correlationId: string): string => (
  `#runtime-events?correlation_id=${encodeURIComponent(correlationId)}`
);

export const usageEventRouteHash = (requestId: string): string => (
  `#events?request_id=${encodeURIComponent(requestId)}`
);

export const modelRouteHash = (model: string): string => `#models?model=${encodeURIComponent(model)}`;

/** 将任意 hash 归一化为规范形式，无法识别时回退到总览。 */
export const canonicalConsoleHash = (hash: string): string => {
  const route = resolveConsoleRoute(hash);
  if (route.page === 'sources' && route.sourceId) return sourceRouteHash(route.sourceId, route.section);
  if (route.page === 'upstream-quotas') return upstreamQuotaRouteHash(route.accountId);
  if (route.page === 'runtime-events' && route.correlationId) return runtimeEventsRouteHash(route.correlationId);
  if (route.page === 'events' && route.requestId) return usageEventRouteHash(route.requestId);
  if (route.page === 'models' && route.modelSearch) return modelRouteHash(route.modelSearch);
  return consolePageHash(route.page);
};
