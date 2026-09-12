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
    'runtime-events': 'Runtime Events',
    sources: 'Sources',
    models: 'Models & Routes',
    settings: 'Settings',
  },

  // Source workspace sub-page titles
  title: {
    source_detail: 'Source Details',
    source_new: 'New Source',
    source_edit: 'Edit Source',
    source_review: 'Model Update Review',
  },

  connection_aria: "Gateway connection",

  // 顶栏
  admin_key_label: 'Admin Key',
  admin_key_placeholder: 'GATEWAY_ADMIN_KEY',

  // 通用占位
  page_loading: 'Loading page…',
} as const;
