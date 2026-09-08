// 模型发现页文案(简体中文)——命名空间 console.discovery
export const discovery = {
  // 字段来源/状态值(值保持枚举原形)
  field_source: {
    user: '用户',
    preset: '预设',
    upstream: '上游',
    unknown: '未知',
  },
  confirm_state: {
    pending: '待确认',
    confirmed: '已确认',
    unavailable: '不可用',
  },
  availability_state: {
    unknown: '未知',
    available: '可用',
    unavailable: '不可用',
  },

  // 公共
  diff_none: '无变化',
  unnamed_metadata: '未命名元数据',
  validate_tokens: 'Token 数值字段必须为空或大于 0。',
  no_field_changes: '没有需要保存的字段变更。',
  none: '无',

  // 运行/结果状态
  empty_run_title: '尚无发现运行',
  empty_run_desc: '运行模型发现以查看结果与差异。',
  run_meta: '{{duration}} ms · {{count}} 个模型',
  run_badge: '运行 #{{id}}',
  account_http: '账号 {{account}} · HTTP {{http}}',
  state_unsupported: '该提供商预设明确不支持模型发现。',
  declares_unsupported: '该提供商预设将模型发现声明为不支持。',
  state_failed: '模型发现失败',
  state_failed_desc: '检查来源连接与账号凭据后重试。',
  state_empty: '发现成功,但上游返回空模型列表',
  state_empty_desc: '现有模型保持不变。',

  // 消息
  message_saved: '来源模型 {{name}} 的用户字段已保存。',
  message_confirmed: '已确认 {{count}} 个来源模型。',

  // 主页面
  loading: '正在加载模型发现…',
  no_sources_title: '没有可用于模型发现的来源',
  no_sources_desc: '先在来源页面创建来源与账号。',
  source: '来源',
  account: '账号',
  confirmation_filter: '确认状态',
  availability_filter: '可用状态',
  no_enabled_account: '没有启用账号',
  run_button: '运行发现',
  latest_run_card: '最近一次发现',
  loading_run: '正在加载最近一次发现…',
  source_model_count: '{{count}} 个来源模型',
  loading_models: '正在加载来源模型…',
  empty_no_auto: '该来源不支持自动发现',
  empty_no_match: '当前筛选没有来源模型',
  empty_no_match_desc: '发现失败，请检查连接后重试。',
  empty_no_auto_desc: '运行模型发现或调整筛选条件。',
  models_card: '来源模型',
  selected_count: '已选 {{count}}',
  confirm_selected: '批量确认',
  edit_pending_title: '编辑待确认的来源模型',
  save_user_fields: '保存用户字段',
  batch_confirm_title: '批量确认来源模型',
  batch_confirm_desc: '确认选中的 {{count}} 个模型？确认后仍需创建绑定和路由。',
  confirm_models: '确认模型',

  // 协议能力编辑
  capabilities_action: '协议能力',
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
    confirmation: '确认',
    availability: '可用性',
    metadata: '元数据',
    field_source: '字段来源',
    preset_match: '预设匹配',
    last_discovered: '最近发现',
  },
  diff_column: {
    added: '新增',
    changed: '变更',
    missing: '缺失',
  },
  select_all_aria: '选择全部可确认模型',
  select_row_aria: '选择 {{model}}',
  table_aria: '来源模型表格',
} as const;
