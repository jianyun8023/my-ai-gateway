// 系统设置页文案(英文)——命名空间 console.settings
export const settings = {
  // 表单与校验
  key_name_required: 'Key name cannot be empty.',
  key_name: 'Key name',
  allowed_models: 'Allowed models',
  allowed_models_hint: 'Comma-separated; leave empty to allow all models.',

  // 消息
  key_created: 'Virtual Key created and stored encrypted; you can view it again at any time.',
  key_revoked: 'Virtual Key {{name}} revoked.',
  runtime_reloaded: 'Runtime snapshot reloaded from PostgreSQL.',
  export_done: 'Exported the current admin resources snapshot; the file contains no credential references.',
  key_cleared: 'The Admin Key for this tab has been cleared.',
  api_key_copied: 'API Key copied to clipboard.',
  api_key_copy_denied: 'The browser did not allow automatic copying.',

  // 主页面
  loading: 'Loading settings…',
  resources_card: 'Admin resources',
  resources_subtitle: 'Live control-plane contract',
  runtime_revision: 'runtime revision {{revision}}',
  card: {
    key_session: 'Admin Key Session',
    key_session_subtitle: 'Stored only in this tab\u2019s sessionStorage and used solely for /admin/* authorization',
    key_configured: 'Admin Key configured for this tab',
    key_not_configured: 'No Admin Key configured for this tab',
    key_hint: 'Enter an Admin Key above to reach the control-plane API.',
    key_loaded_hint: 'This page loaded successfully, which only means the Admin API is currently reachable; it does not imply that backend authentication is enabled.',
    runtime: 'Runtime Snapshot',
    runtime_subtitle: 'Fact source: GET /admin/capabilities; reload: POST /admin/config/reload',
    export: 'Configuration Export',
    export_subtitle: 'Combines current Sources, Accounts, LogicalModels, Bindings, Routes, and the runtime revision',
    export_redacted: 'Redacted JSON',
    export_redacted_hint: 'Credential references are replaced with placeholders; no secrets are exported.',
    keys: 'Virtual Keys',
    keys_subtitle: 'Authentication uses an irreversible hash; raw values are stored encrypted and require the Admin Key to view or copy',
  },
  clear_key: 'Clear session key',
  reload_runtime: 'Reload Runtime',
  export_json: 'Export JSON',
  new_key: 'New Key',

  // 运行时快照字段
  snapshot_field: {
    revision: 'Revision',
    generated_at: 'Generated at',
    fact_source: 'Fact source',
    published_rows: 'Published rows',
  },

  // 虚拟密钥表
  keys_empty: 'No virtual keys yet',
  keys_table_aria: 'Virtual keys table',
  keys_column: {
    name: 'Name',
    prefix: 'Prefix',
    allowed_models: 'Allowed models',
    created: 'Created',
    last_used: 'Last used',
    status: 'Status',
  },
  all_models: 'All models',
  row_id: 'ID {{id}}',
  key_status: {
    active: 'Active',
    disabled: 'Disabled',
    revoked: 'Revoked',
  },
  view_key_aria: 'View {{name}} API Key',
  view_key_unavailable_aria: '{{name}} cannot be viewed; rotate it',
  revoke_key_aria: 'Revoke {{name}}',

  // 弹窗
  modal: {
    new_key: 'New Virtual Key',
    create_key: 'Create Key',
    reveal_title: 'Virtual Key · {{name}}',
    reveal_subtitle: 'API Key can be viewed and copied now.',
    reveal_decrypt_note: 'This value is returned decrypted only through the Admin API; data-plane authentication still uses the irreversible hash stored in the database.',
    revoke_title: 'Revoke Virtual Key',
    revoke_body: 'Revoke {{name}}? Revoked keys cannot be restored.',
    revoke_confirm: 'Revoke',
  },

  // 模型元数据编辑器(Models & Routes / Discovery 复用)
  metadata: {
    field: {
      logical_model_name: 'Suggested logical model name',
      display_name: 'Display name',
      context_window: 'Context window',
      max_input_tokens: 'Max input tokens',
      max_output_tokens: 'Max output tokens',
      input_modalities: 'Input modalities',
      output_modalities: 'Output modalities',
      tools: 'Tools',
      thinking: 'Thinking',
      web_search: 'Web Search',
      structured_output: 'Structured Output',
      streaming: 'Streaming',
      usage: 'Usage',
    },
    feature: {
      unknown: 'Unknown',
      supported: 'Supported',
      unsupported: 'Unsupported',
    },
  },
} as const;
