// 系统设置页文案(英文)——命名空间 console.settings
export const settings = {
  overlap_seconds: "Overlap (seconds)",
  overlap_hint: "0–86400 seconds. The old key remains valid during overlap, subject to its original expiry.",
  overlap_invalid: "Enter a whole number between 0 and 86400 seconds.",
  rotation_hint: "Generate a new key and set its model allowlist. The old key keeps its existing model permissions.",
  rotation_immediate: "With no overlap, the old key stops working immediately after rotation.",
  key_rotated_overlap: "{{name}} rotated. The old key is valid until at most {{until}}. Update your clients before then.",
  key_rotated_immediate: "{{name}} rotated. The old key is no longer valid. Update your client key.",
  valid_until: "Valid until at most {{until}}",
  rotate_key_aria: "Rotate {{name}}",
  // 表单与校验
  key_name_required: 'Key name cannot be empty.',
  key_name: 'Key name',
  allowed_models: 'Allowed models',
  allowed_models_hint: 'Comma-separated; leave empty to allow all models.',

  // 消息
  key_created: 'Virtual key created. You can view or copy it.',
  key_revoked: 'Virtual Key {{name}} revoked.',
  runtime_reloaded: 'Runtime configuration reloaded.',
  export_done: 'Configuration exported.',
  key_cleared: 'The Admin Key for this tab has been cleared.',
  api_key_copied: 'API Key copied to clipboard.',
  api_key_copy_denied: 'The browser did not allow automatic copying.',

  // 主页面
  loading: 'Loading settings…',
  resources_card: 'Admin resources',
  runtime_revision: 'runtime revision {{revision}}',
  card: {
    key_session: 'Admin connection',
    key_session_subtitle: 'The Admin Key is stored only in this tab.',
    key_configured: 'Admin Key configured for this tab',
    key_not_configured: 'No Admin Key configured for this tab',
    key_hint: 'Enter an Admin Key in Gateway connection, or open navigation on smaller screens.',
    runtime: 'Runtime Snapshot',
    export: 'Configuration Export',
    export_redacted: 'Redacted JSON',
    export_redacted_hint: 'Includes sources, accounts, models and routes, without keys.',
    keys: 'Virtual Keys',
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
    expired: "Expired",
    overlap: "Overlap",
    rotated: "Rotated",
    active: 'Active',
    disabled: 'Disabled',
    revoked: 'Revoked',
  },
  view_key_aria: 'View {{name}} API Key',
  view_key_unavailable_aria: '{{name}} cannot be viewed; rotate it',
  revoke_key_aria: 'Revoke {{name}}',

  // 弹窗
  modal: {
    rotate_title: "Rotate key · {{name}}",
    rotate_confirm: "Rotate Key",
    new_key: 'New Virtual Key',
    create_key: 'Create Key',
    reveal_title: 'Virtual Key · {{name}}',
    reveal_subtitle: 'API Key can be viewed and copied now.',
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
