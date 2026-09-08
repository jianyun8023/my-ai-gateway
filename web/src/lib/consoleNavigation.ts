export const CONSOLE_PAGES = [
  'overview', 'analysis', 'events', 'runtime-events',
  'sources', 'discovery', 'models', 'capabilities', 'settings',
] as const;

export type ConsolePage = typeof CONSOLE_PAGES[number];
export type GatewayUsageTab = Extract<ConsolePage, 'overview' | 'analysis' | 'events'>;
export type GatewayManagementPage = Exclude<ConsolePage, GatewayUsageTab>;
export interface ConsoleNavSection { label: string; pages: readonly ConsolePage[] }

export function isUsagePage(page: ConsolePage): page is GatewayUsageTab {
  return page === 'overview' || page === 'analysis' || page === 'events';
}

export const resolveConsolePage = (hash: string): ConsolePage => {
  const normalized = hash.trim().replace(/^#/, '').replace(/^\/+/, '').replace(/\/+$/, '');
  return CONSOLE_PAGES.includes(normalized as ConsolePage) ? normalized as ConsolePage : 'overview';
};

export const consolePageHash = (page: ConsolePage): string => '#' + page;
