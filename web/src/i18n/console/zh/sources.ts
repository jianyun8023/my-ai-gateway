// 来源管理页文案(简体中文)——命名空间 console.sources
export const sources = {
  // 模式/能力状态(值保持枚举原形)
  mode: {
    native: '原生',
    adapter: '转换',
    unsupported: '不支持',
    unknown: '未知',
  },

  // 凭据状态标签
  credential: {
    configured: '已配置',
    not_configured: '未配置',
  },

  // 表单校验
  form: {
    validate_required_source: '来源 ID、显示名称、提供商预设与 Base URL 均为必填项。',
    validate_preset: '请选择有效的提供商预设版本。',
    validate_adapter_mode: '转换模式必须同时指定上游协议与适配器名称。',
    validate_headers_json: '默认请求头必须是有效的 JSON 对象。',
    validate_headers_object: '默认请求头必须是 JSON 对象。',
    validate_headers_nonempty: '凭据请求头不能为空。',
    validate_headers_string: '默认请求头的值必须是字符串。',
    validate_required_account: '账号 ID、来源、显示名称与凭据环境变量均为必填项。',
    validate_weight: '回退权重必须是正整数。',
    section_capabilities: '三协议能力声明',
    mode: '模式',
    upstream_protocol: '上游协议',
    adapter: '适配器',
    section_auth: '认证模板',
    credential_headers: '凭据请求头',
    header_prefix: '请求头前缀',
    default_headers: '默认请求头(JSON)',
    no_preset: '无可用预设',
    endpoint_hint: '{{protocol}} 端点',
    credential_env_hint_no_echo: '现有引用不会回显;如需保存请重新输入环境变量名。',
    credential_env_hint_name_only: '仅提交环境变量名,不提交凭据值。',
    verify_on_submit: '提交后验证',
  },

  // 字段
  field: {
    source_id: '来源 ID',
    display_name: '显示名称',
    provider_preset: '提供商预设',
    base_url: 'Base URL',
    enable_source: '启用来源',
    account_id: '账号 ID',
    source: '来源',
    credential_env: '凭据环境变量',
    credential_status: '凭据状态',
    fallback_weight: '回退权重',
    enable_account: '启用账号',
  },

  // 详情抽屉
  detail: {
    title_source: '来源详情',
    basic_info: '基本信息',
    protocol_snapshot: '协议快照',
    endpoint_unset: '未配置端点',
    upstream_endpoint: '上游端点',
    preset_diff: '提供商预设差异',
    preset_comparing: '正在比较预设…',
    preset_same: '当前快照与最新预设一致',
    preset_versions: 'v{{from}} → v{{to}}',
    diff_count: '{{count}} 项差异',
    diff_path: '路径',
    diff_type: '类型',
    diff_old: '原值',
    diff_new: '新值',
    connection_test: '三协议连接测试',
    test_no_account: '没有可用账号',
    test_no_account_desc: '先创建并启用属于此来源的账号。',
    test_model: '测试模型(可选)',
    test_ok: '连接成功',
    test_failed: '连接失败',
    test_button: '测试',
    test_no_http: '无 HTTP',
  },

  // 主页面
  loading: '正在加载来源与账号…',
  region_aria: '来源与账号管理',
  tab: {
    sources: '来源',
    accounts: '账号',
  },
  empty: {
    sources_title: '尚未配置来源',
    sources_desc: '从提供商预设创建第一个来源。',
    accounts_title: '尚未配置账号',
    accounts_desc: '选择来源并添加账号凭据。',
  },
  add_source: '新增来源',
  add_account: '新增账号',
  card: {
    sources_title: '来源',
    accounts_title: '账号',
  },
  table: {
    header_credentials: '凭据',
    header_fallback_weight: '回退权重',
    header_health: '健康状态',
    header_cooldown: '冷却',
    toggle_aria: '启停 {{id}}',
    toggle_enabled: '来源 {{name}} 已启用。',
    toggle_disabled: '来源 {{name}} 已停用。',
    view_aria: '查看 {{id}}',
    edit_aria: '编辑 {{id}}',
    delete_aria: '删除 {{id}}',
    account_toggle_enabled: '账号 {{name}} 已启用。',
    account_toggle_disabled: '账号 {{name}} 已停用。',
    sources_region: '来源表格',
    accounts_region: '账号表格',
  },

  // 弹窗与消息
  modal: {
    edit_source: '编辑来源',
    new_source: '新增来源',
    edit_account: '编辑账号',
    new_account: '新增账号',
  },
  message: {
    source_created: '来源 {{name}} 已创建。',
    source_updated: '来源 {{name}} 已更新。',
    source_deleted: '来源 {{name}} 已删除。',
    account_created: '账号 {{name}} 已创建。',
    account_updated: '账号 {{name}} 已更新。',
    account_deleted: '账号 {{name}} 已删除。',
  },
  confirm: {
    delete_source_title: '删除来源',
    delete_source_body: '确认删除 {{id}}？请先移除关联资源。',
    delete_account_title: '删除账号',
    delete_account_body: '确认删除 {{id}}？请先移除关联资源。',
  },
} as const;
