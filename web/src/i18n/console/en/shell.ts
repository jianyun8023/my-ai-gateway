// 控制台外壳/导航文案(英文)——命名空间 console.shell
export const shell = {
  appearance: {
    title: 'Appearance',
    description: 'Preview a theme style and display mode instantly.',
    trigger: 'Appearance: {{style}}, {{mode}}',
    style_label: 'Theme style',
    mode_label: 'Display mode',
    modes: { auto: 'System', light: 'Light', dark: 'Dark' },
    styles: {
      utility: { label: 'Console Green', description: 'Cool gray engineering workspace' },
      ocean: { label: 'Deep Ocean', description: 'Blue-gray observability console' },
      nebula: { label: 'Nebula', description: 'Modern indigo intelligence interface' },
      sandstone: { label: 'Sandstone', description: 'Warm neutral operations interface' },
    },
  },
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
    'upstream-quotas': 'Upstream Quotas',
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
  endpoint_label: 'Gateway address',
  key_configured: 'An Admin Key is configured for this tab.',
  key_not_configured: 'Enter an Admin Key to access management features.',

  // 顶栏
  admin_key_label: 'Admin Key',
  admin_key_placeholder: 'GATEWAY_ADMIN_KEY',

  // 通用占位
  page_loading: 'Loading page…',
} as const;
