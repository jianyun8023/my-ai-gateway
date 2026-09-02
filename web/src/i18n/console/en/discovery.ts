// 模型发现页文案(英文)——命名空间 console.discovery
export const discovery = {
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
  empty_run_desc: 'Run discovery to see audit status and a stable diff here.',
  run_meta: '{{duration}} ms · {{count}} models',
  run_badge: 'Run #{{id}}',
  account_http: 'Account {{account}} · HTTP {{http}}',
  state_unsupported: 'This provider preset does not support model discovery.',
  declares_unsupported: 'This provider preset declares model discovery as unsupported.',
  state_failed: 'Model discovery failed',
  state_failed_desc: 'The upstream discovery did not succeed.',
  state_empty: 'Discovery succeeded but the upstream returned no models',
  state_empty_desc: 'Existing SourceModels were neither fabricated nor auto-deleted.',
  state_ok: 'Discovery run #{{id}} finished; {{count}} models discovered.',
  state_failed_recorded: 'Discovery recorded failed run #{{id}}.',

  // 消息
  message_saved: 'User fields for SourceModel {{name}} saved.',
  message_confirmed: '{{count}} SourceModels confirmed.',
  message_unsupported: 'Discovery unsupported: {{code}}',

  // 主页面
  loading: 'Loading discovery context…',
  no_sources_title: 'No Sources available for discovery',
  no_sources_desc: 'Create a Source and Account in the Sources page first.',
  source: 'Source',
  account: 'Account',
  confirmation_filter: 'Confirmation status',
  availability_filter: 'Availability status',
  no_enabled_account: 'No enabled Account',
  run_button: 'Run Discovery',
  latest_run_card: 'Latest Run',
  latest_run_subtitle: 'Audit run, upstream result, and added / changed / missing differences',
  loading_run: 'Loading latest run…',
  source_model_count: '{{count}} SourceModels',
  loading_models: 'Loading SourceModels…',
  empty_no_auto: 'This Source does not support automatic discovery',
  empty_no_match: 'No SourceModels match the current filters',
  empty_no_match_desc: 'The latest discovery failed, so existing SourceModels were not modified.',
  empty_no_auto_desc: 'Run discovery or adjust the filters.',
  models_card: 'SourceModels',
  models_subtitle: 'Confirmation only updates SourceModels; it never creates LogicalModels, Bindings, or Routes',
  selected_count: '{{count}} selected',
  confirm_selected: 'Confirm Selected',
  edit_pending_title: 'Edit Pending SourceModel',
  save_user_fields: 'Save User Fields',
  batch_confirm_title: 'Confirm SourceModels',
  batch_confirm_desc: 'Confirm the {{count}} selected available pending SourceModels. This never implicitly creates LogicalModels, Bindings, or Routes.',
  confirm_models: 'Confirm Models',

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
