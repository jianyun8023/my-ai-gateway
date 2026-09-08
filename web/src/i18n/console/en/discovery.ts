// 模型发现页文案(英文)——命名空间 console.discovery
export const discovery = {
  filters_aria: 'Model discovery filters',
  // 字段来源/状态值(值保持枚举原形)
  field_source: {
    user: 'User',
    preset: 'Preset',
    upstream: 'Upstream',
    unknown: 'Unknown',
  },
  confirm_state: {
    pending: 'Pending',
    confirmed: 'Confirmed',
    unavailable: 'Unavailable',
  },
  availability_state: {
    unknown: 'Unknown',
    available: 'Available',
    unavailable: 'Unavailable',
  },

  // 公共
  diff_none: 'No changes',
  unnamed_metadata: 'Unnamed metadata',
  validate_tokens: 'Token numeric fields must be empty or greater than 0.',
  no_field_changes: 'No field changes to save.',
  none: 'none',

  // 运行/结果状态
  empty_run_title: 'No discovery run yet',
  empty_run_desc: 'Run discovery to see models and changes.',
  run_meta: '{{duration}} ms · {{count}} models',
  run_badge: 'Run #{{id}}',
  account_http: 'Account {{account}} · HTTP {{http}}',
  state_unsupported: 'This provider preset does not support model discovery.',
  declares_unsupported: 'This provider preset declares model discovery as unsupported.',
  state_failed: 'Model discovery failed',
  state_failed_desc: 'Check the source connection and account credentials, then retry.',
  state_empty: 'Discovery succeeded but the upstream returned no models',
  state_empty_desc: 'Existing models are unchanged.',

  // 消息
  message_saved: 'User fields for SourceModel {{name}} saved.',
  message_confirmed: '{{count}} SourceModels confirmed.',

  // 主页面
  loading: 'Loading model discovery…',
  no_sources_title: 'No Sources available for discovery',
  no_sources_desc: 'Create a Source and Account in the Sources page first.',
  source: 'Source',
  account: 'Account',
  confirmation_filter: 'Confirmation status',
  availability_filter: 'Availability status',
  no_enabled_account: 'No enabled Account',
  run_button: 'Run Discovery',
  latest_run_card: 'Latest Run',
  loading_run: 'Loading latest run…',
  source_model_count: '{{count}} SourceModels',
  loading_models: 'Loading SourceModels…',
  empty_no_auto: 'This Source does not support automatic discovery',
  empty_no_match: 'No SourceModels match the current filters',
  empty_no_match_desc: 'Discovery failed. Check the connection and retry.',
  empty_no_auto_desc: 'Run discovery or adjust the filters.',
  models_card: 'SourceModels',
  selected_count: '{{count}} selected',
  confirm_selected: 'Confirm Selected',
  edit_pending_title: 'Edit Pending SourceModel',
  save_user_fields: 'Save User Fields',
  batch_confirm_title: 'Confirm SourceModels',
  batch_confirm_desc: 'Confirm {{count}} selected models? Bindings and routes must be created separately.',
  confirm_models: 'Confirm Models',

  // Protocol capability editing
  capabilities_action: 'Capabilities',
  capabilities_title: 'Protocol capabilities — {{model}}',
  capabilities_subtitle: 'Declare how each ingress protocol is handled; bindings require a confirmed capability.',
  capability_loading: 'Loading protocol capabilities…',
  capability_undeclared: 'Undeclared',
  capability_mode: 'Mode',
  capability_status: 'Status',
  capability_source_protocol: 'Upstream protocol',
  capability_adapter: 'Adapter',
  capability_save: 'Save capability',
  capability_saved: '{{protocol}} capability saved.',
  capability_unknown_confirm_hint: 'Unknown capabilities cannot be confirmed.',
  capability_adapter_hint: 'Adapter mode requires a confirmed native upstream protocol.',

  // 表格
  column: {
    upstream_model: 'Upstream model',
    confirmation: 'Confirmation',
    availability: 'Availability',
    metadata: 'Metadata',
    field_source: 'Field source',
    preset_match: 'Preset match',
    last_discovered: 'Last discovered',
  },
  diff_column: {
    added: 'Added',
    changed: 'Changed',
    missing: 'Missing',
  },
  select_all_aria: 'Select all confirmable models',
  select_row_aria: 'Select {{model}}',
  table_aria: 'SourceModels table',
} as const;
