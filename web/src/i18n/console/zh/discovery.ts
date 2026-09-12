// 模型更新审核文案(简体中文)——命名空间 console.discovery
export const discovery = {
  filters_aria: '模型更新审核筛选',
  // 字段来源/状态值(值保持枚举原形)
  field_source: {
    user: '用户',
    preset: '预设',
    upstream: '上游',
    unknown: '未知',
  },
  confirm_state: {
    pending: '待审核',
    confirmed: '已确认',
    unavailable: '标记不可用',
  },
  availability_state: {
    unknown: '未知',
    available: '可用',
    unavailable: '不可用',
  },
  // 发现运行状态
  run_state: {
    succeeded: '运行成功',
    failed: '运行失败',
    unsupported: '不支持',
  },
  // 发现变化类型
  change_kind: {
    added: '新增模型',
    changed: '字段变化',
    changed_with_fields: '字段变化: {{fields}}',
    missing: '缺失',
    unknown: '未取得比较结果',
  },
  // 能力摘要特性
  feature: {
    tools: 'Tools',
    thinking: '推理',
    web_search: 'Web Search',
    structured_output: '结构化输出',
  },

  // 公共
  diff_none: '无变化',
  unnamed_metadata: '未命名元数据',
  validate_tokens: 'Token 数值字段必须为空或大于 0。',
  no_field_changes: '没有需要保存的字段变更。',
  none: '无',

  // 运行/结果状态
  loading_run: '正在加载最近一次模型更新…',
  state_unsupported: '该提供商预设不支持自动同步上游模型列表，无法执行模型更新检查。',
  state_failed: '模型更新检查失败',
  state_failed_desc: '检查来源连接与账号凭据后重试。',
  state_empty: '检查成功，但上游返回空模型列表',
  state_empty_desc: '现有模型保持不变。',

  // 消息
  message_saved: '来源模型 {{name}} 的用户字段已保存。',
  message_confirmed: '已确认 {{count}} 个来源模型。',

  // 主页面
  loading: '正在加载模型更新审核…',
  confirmation_filter: '确认状态',
  availability_filter: '可用状态',
  source_model_count: '{{count}} 个来源模型',
  loading_models: '正在加载来源模型…',
  empty_no_auto: '该来源不支持自动检查模型更新',
  empty_no_match: '当前筛选没有来源模型',
  empty_no_match_desc: '检查失败，请检查连接后重试。',
  empty_no_auto_desc: '运行模型更新检查或调整筛选条件。',
  models_card: '上游模型',
  confirm_selected: '批量确认',
  edit_pending_title: '编辑待审核的来源模型',
  save_user_fields: '保存用户字段',
  batch_confirm_title: '批量确认来源模型',
  batch_confirm_desc: '确认选中的 {{count}} 个模型？确认后模型可用于模型与路由，但仍需自行创建绑定和路由。',
  confirm_models: '确认模型',

  // 协议能力编辑
  capabilities_action: '编辑能力',
  capabilities_title: '协议能力 — {{model}}',
  capabilities_subtitle: '声明每个北向协议的处理方式;绑定要求能力已确认。',
  capability_loading: '正在加载协议能力…',
  capability_undeclared: '未声明',
  capability_mode: '处理方式',
  capability_status: '确认状态',
  capability_source_protocol: '上游协议',
  capability_adapter: '转换器',
  capability_save: '保存能力',
  capability_saved: '{{protocol}} 能力已保存。',
  capability_unknown_confirm_hint: '未知能力不能被确认。',
  capability_adapter_hint: '转换模式要求上游协议已确认为原生。',

  // 表格
  column: {
    upstream_model: '上游模型',
    confirmation: '确认状态',
    availability: '可用性',
  },
  diff_column: {
    added: '新增',
    changed: '变更',
    missing: '缺失',
  },
  select_all_aria: '选择全部可确认模型',
  select_row_aria: '选择 {{model}}',
  table_aria: '上游模型审核表格',
} as const;
