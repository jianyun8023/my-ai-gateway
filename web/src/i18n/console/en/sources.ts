// 来源管理页文案(英文)——命名空间 console.sources
export const sources = {
  // 模式/能力状态(值保持枚举原形)
  mode: {
    native: 'Native',
    adapter: 'Adapter',
    unsupported: 'Unsupported',
    unknown: 'Unknown',
  },

  // 凭据状态标签
  credential: {
    configured: 'Configured',
    not_configured: 'Not configured',
  },

  // 表单校验
  form: {
    validate_required_source: 'Source ID, display name, provider preset, and Base URL are required.',
    validate_preset: 'Please select a valid provider preset version.',
    validate_adapter_mode: 'Adapter mode requires an upstream protocol and an adapter name.',
    validate_headers_json: 'Default headers must be a valid JSON object.',
    validate_headers_object: 'Default headers must be a JSON object.',
    validate_headers_nonempty: 'Credential headers cannot be empty.',
    validate_headers_string: 'Default header values must be strings.',
    validate_required_account: 'Account ID, Source, display name, and credential environment variable are required.',
    validate_weight: 'Fallback weight must be a positive integer.',
    section_capabilities: 'Protocol capability declarations',
    mode: 'Mode',
    upstream_protocol: 'Upstream protocol',
    adapter: 'Adapter',
    section_auth: 'Authentication template',
    credential_headers: 'Credential headers',
    header_prefix: 'Header prefix',
    default_headers: 'Default headers (JSON)',
    no_preset: 'No presets available',
    endpoint_hint: '{{protocol}} endpoint',
    credential_env_hint_no_echo: 'Existing references are not echoed; re-enter the variable name to save it.',
    credential_env_hint_name_only: 'Only the variable name is submitted, never the credential value.',
    verify_on_submit: 'Verify on submit',
  },

  // 字段
  field: {
    source_id: 'Source ID',
    display_name: 'Display name',
    provider_preset: 'Provider preset',
    base_url: 'Base URL',
    enable_source: 'Enable Source',
    account_id: 'Account ID',
    source: 'Source',
    credential_env: 'Credential environment variable',
    credential_status: 'Credential status',
    fallback_weight: 'Fallback weight',
    enable_account: 'Enable Account',
  },

  // 详情抽屉
  detail: {
    title_source: 'Source Details',
    basic_info: 'Basic information',
    protocol_snapshot: 'Protocol snapshot',
    endpoint_unset: 'Endpoint not configured',
    upstream_endpoint: 'upstream endpoint',
    preset_diff: 'Provider preset differences',
    preset_comparing: 'Comparing presets…',
    preset_same: 'The current snapshot matches the latest preset',
    preset_versions: 'v{{from}} → v{{to}}',
    diff_count: '{{count}} differences',
    diff_path: 'Path',
    diff_type: 'Type',
    diff_old: 'Old value',
    diff_new: 'New value',
    connection_test: 'Connection test across protocols',
    test_no_account: 'No accounts available',
    test_no_account_desc: 'Create and enable an account for this source first.',
    test_model: 'Test model (optional)',
    test_ok: 'Succeeded',
    test_failed: 'Failed',
    test_button: 'Test',
    test_no_http: 'no HTTP',
  },

  // 主页面
  loading: 'Loading sources and accounts…',
  region_aria: 'Source and account resources',
  tab: {
    sources: 'Sources',
    accounts: 'Accounts',
  },
  empty: {
    sources_title: 'No sources configured yet',
    sources_desc: 'Create your first source from a provider preset.',
    accounts_title: 'No accounts configured yet',
    accounts_desc: 'Select a source and add account credentials.',
  },
  add_source: 'New Source',
  add_account: 'New Account',
  card: {
    sources_title: 'Sources',
    accounts_title: 'Accounts',
  },
  table: {
    header_credentials: 'Credentials',
    header_fallback_weight: 'Fallback weight',
    header_health: 'Health',
    header_cooldown: 'Cooldown',
    toggle_aria: 'Toggle {{id}}',
    toggle_enabled: 'Source {{name}} has been enabled.',
    toggle_disabled: 'Source {{name}} has been disabled.',
    view_aria: 'View {{id}}',
    edit_aria: 'Edit {{id}}',
    delete_aria: 'Delete {{id}}',
    account_toggle_enabled: 'Account {{name}} has been enabled.',
    account_toggle_disabled: 'Account {{name}} has been disabled.',
    sources_region: 'Sources table',
    accounts_region: 'Accounts table',
  },

  // 弹窗与消息
  modal: {
    edit_source: 'Edit Source',
    new_source: 'New Source',
    edit_account: 'Edit Account',
    new_account: 'New Account',
  },
  message: {
    source_created: 'Source {{name}} created.',
    source_updated: 'Source {{name}} updated.',
    source_deleted: 'Source {{name}} deleted.',
    account_created: 'Account {{name}} created.',
    account_updated: 'Account {{name}} updated.',
    account_deleted: 'Account {{name}} deleted.',
  },
  confirm: {
    delete_source_title: 'Delete Source',
    delete_source_body: 'Delete {{id}}? Remove linked resources first.',
    delete_account_title: 'Delete Account',
    delete_account_body: 'Delete {{id}}? Remove linked resources first.',
  },
} as const;
