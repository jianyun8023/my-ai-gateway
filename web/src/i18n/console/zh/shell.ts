// 控制台外壳/导航文案(简体中文)——命名空间 console.shell
export const shell = {
  // 品牌与侧栏
  brand_name: 'AI Gateway',
  sidebar_aria: '控制台侧栏',
  nav_aria: '主导航',
  close_nav: '关闭导航',
  close_overlay: '关闭导航遮罩',
  open_nav: '打开导航',
  mobile_key_label: 'Admin Key',
  mobile_key_apply: '应用',
  running_status: '网关运行中',

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
    sources: '来源管理',
    models: '模型与路由',
    settings: '系统设置',
  },

  // 页面描述
  desc: {
    overview: '过去 24 小时的网关运行状态',
    analysis: 'Token 构成、模型分布、来源分析与延迟诊断',
    events: '查看每次请求的元数据、重试与 Token 明细',
    sources: '管理提供商、来源、账号与连接配置',
    models: '逻辑模型映射、来源绑定与协议路由配置',
    settings: '网关入口、虚拟密钥与配置管理',
  },

  // 顶栏
  admin_key_label: 'Admin Key',
  admin_key_placeholder: 'GATEWAY_ADMIN_KEY',
  search: '搜索…',
  search_hint: '⌘K',

  // 通用占位
  page_loading: '正在加载页面…',
} as const;
