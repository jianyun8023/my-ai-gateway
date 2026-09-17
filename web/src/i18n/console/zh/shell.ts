// 控制台外壳/导航文案(简体中文)——命名空间 console.shell
export const shell = {
  appearance: {
    title: '外观设置',
    description: '即时预览主题风格与显示模式。',
    trigger: '外观设置：{{style}}，{{mode}}',
    style_label: '主题风格',
    mode_label: '显示模式',
    modes: { auto: '跟随系统', light: '浅色', dark: '深色' },
    styles: {
      utility: { label: '青绿控制台', description: '冷灰与绿色的工程工作台' },
      ocean: { label: '深海观测', description: '蓝灰与青色的观测控制台' },
      nebula: { label: '星云', description: '靛紫色的现代智能界面' },
      sandstone: { label: '砂岩', description: '暖灰与琥珀的沉稳界面' },
    },
  },
  // 品牌与侧栏
  brand_name: 'AI Gateway',
  sidebar_aria: '控制台侧栏',
  nav_aria: '主导航',
  close_nav: '关闭导航',
  close_overlay: '关闭导航遮罩',
  open_nav: '打开导航',

  // 导航分组
  section: {
    monitor: '监控',
    config: '配置',
    system: '系统',
  },

  // 页面标题(导航项/页头共用)
  nav: {
    overview: '总览',
    analysis: '用量分析',
    events: '请求事件',
    'upstream-quotas': '上游额度',
    'runtime-events': '运行事件',
    sources: '来源管理',
    models: '模型与路由',
    settings: '系统设置',
  },

  // 来源管理工作区子页面标题
  title: {
    source_detail: '来源详情',
    source_new: '新增来源',
    source_edit: '编辑来源',
    source_review: '模型更新审核',
  },

  connection_aria: "网关连接",
  endpoint_label: '网关地址',
  key_configured: '当前标签页已配置 Admin Key。',
  key_not_configured: '填写 Admin Key 以访问管理功能。',

  // 顶栏
  admin_key_label: 'Admin Key',
  admin_key_placeholder: 'GATEWAY_ADMIN_KEY',

  // 通用占位
  page_loading: '正在加载页面…',
} as const;
