// 控制台外壳/导航文案(英文)——命名空间 console.shell
export const shell = {
  switch_to_light: 'Switch to light theme',
  switch_to_dark: 'Switch to dark theme',
  // 品牌与侧栏
  brand_name: 'AI Gateway',
  sidebar_aria: 'Console sidebar',
  nav_aria: 'Main navigation',
  close_nav: 'Close navigation',
  close_overlay: 'Close navigation overlay',
  open_nav: 'Open navigation',
  mobile_key_label: 'Admin Key',
  mobile_key_apply: 'Apply',
  running_status: 'Gateway running',

  // 导航分组
  section: {
    monitor: 'Monitor',
    config: 'Configuration',
    system: 'System',
  },

  // 页面标题(导航项/页头共用)
  nav: {
    overview: 'Overview',
    analysis: 'Analysis',
    events: 'Request Events',
    sources: 'Sources',
    discovery: 'Model Discovery',
    models: 'Models & Routes',
    capabilities: 'Capabilities',
    settings: 'Settings',
  },

  // 页面描述
  desc: {
    overview: 'Gateway operating status for the last 24 hours',
    analysis: 'Token mix, model and source distribution, and latency diagnostics',
    events: 'Inspect request metadata, retries, and token details',
    sources: 'Manage providers, sources, accounts, and connection configuration',
    discovery: 'Discover upstream models, review diffs, and confirm before routing',
    models: 'Logical model mapping, source bindings, and protocol routing',
    capabilities: 'Effective three-protocol capability matrix per source and protocol',
    settings: 'Gateway entry points, virtual keys, and configuration',
  },

  // 顶栏
  admin_key_label: 'Admin Key',
  admin_key_placeholder: 'GATEWAY_ADMIN_KEY',
  search: 'Search…',
  search_hint: '⌘K',

  // 通用占位
  page_loading: 'Loading page…',
} as const;
