// 控制台外壳/导航文案(简体中文)——命名空间 console.shell
export const shell = {
  switch_to_light: '切换为浅色主题',
  switch_to_dark: '切换为深色主题',
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
    'runtime-events': '运行事件',
    sources: '来源管理',
    models: '模型与路由',
    capabilities: '能力矩阵',
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

  // 顶栏
  admin_key_label: 'Admin Key',
  admin_key_placeholder: 'GATEWAY_ADMIN_KEY',

  // 通用占位
  page_loading: '正在加载页面…',
} as const;
