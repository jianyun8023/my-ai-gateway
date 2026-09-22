// 系统设置页文案(简体中文)——命名空间 console.settings
export const settings = {
  overlap_seconds: "重叠期（秒）",
  overlap_hint: "0–86400 秒。重叠期内旧密钥仍可使用，原到期时间仍然有效。",
  overlap_invalid: "请输入 0–86400 之间的整数秒数。",
  rotation_hint: "生成新密钥，并为新密钥设置模型白名单；旧密钥的模型权限保持不变。",
  rotation_immediate: "重叠期为 0，轮换后旧密钥立即失效。",
  key_rotated_overlap: "{{name}} 已轮换。旧密钥最晚有效至 {{until}}，请在此前更新客户端。",
  key_rotated_immediate: "{{name}} 已轮换，旧密钥已失效。请更新客户端密钥。",
  valid_until: "最晚有效至 {{until}}",
  rotate_key_aria: "轮换 {{name}}",
  // 表单与校验
  key_name_required: '密钥名称不能为空。',
  key_name: '密钥名称',
  allowed_models: '允许的模型',
  allowed_models_hint: '逗号分隔;留空表示不限制模型。',

  // 消息
  key_created: '虚拟密钥已创建，可查看或复制。',
  key_revoked: '虚拟密钥 {{name}} 已撤销。',
  runtime_reloaded: '运行配置已重新加载。',
  export_done: '配置已导出。',
  key_cleared: '当前标签页的 Admin Key 已清除。',
  api_key_copied: 'API Key 已复制到剪贴板。',
  api_key_copy_denied: '浏览器未允许自动复制。',

  // 主页面
  loading: '正在加载设置…',
  resources_card: '管理资源',
  runtime_revision: '运行时版本 {{revision}}',
  card: {
    key_session: '管理连接',
    key_session_subtitle: 'Admin Key 仅保存在当前标签页。',
    key_configured: '当前标签页已配置 Admin Key',
    key_not_configured: '当前标签页未配置 Admin Key',
    key_hint: '在“网关连接”中填写 Admin Key；窄屏下请打开导航。',
    runtime: '运行时快照',
    export: '配置导出',
    export_redacted: '脱敏 JSON',
    export_redacted_hint: '包含来源、账号、模型和路由，不含密钥。',
    keys: '虚拟密钥',
  },
  clear_key: '清除会话密钥',
  reload_runtime: '重新加载运行时',
  export_json: '导出 JSON',
  new_key: '新建密钥',

  // 运行时快照字段
  snapshot_field: {
    revision: '版本',
    generated_at: '生成时间',
    fact_source: '配置来源',
    published_entries: '能力矩阵',
  },
  snapshot_value: {
    runtime_snapshot: '当前生效的运行配置',
    published_entries: '{{count}} 条能力条目',
  },

  // 虚拟密钥表
  keys_empty: '尚无虚拟密钥',
  keys_table_aria: '虚拟密钥表格',
  keys_column: {
    name: '名称',
    prefix: '前缀',
    allowed_models: '允许模型',
    created: '创建时间',
    last_used: '最近使用',
    status: '状态',
  },
  all_models: '全部模型',
  row_id: 'ID {{id}}',
  key_status: {
    expired: "已过期",
    overlap: "重叠期",
    rotated: "已轮换",
    active: '已启用',
    disabled: '已停用',
    revoked: '已撤销',
  },
  view_key_aria: '查看 {{name}} 的 API Key',
  view_key_unavailable_aria: '{{name}} 不可查看,需轮换',
  revoke_key_aria: '撤销 {{name}}',

  // 弹窗
  modal: {
    rotate_title: "轮换密钥 · {{name}}",
    rotate_confirm: "轮换密钥",
    new_key: '新建虚拟密钥',
    create_key: '创建密钥',
    reveal_title: '虚拟密钥 · {{name}}',
    reveal_subtitle: 'API Key 当前可查看和复制。',
    revoke_title: '撤销虚拟密钥',
    revoke_body: '确认撤销 {{name}}?已撤销的密钥不能恢复。',
    revoke_confirm: '撤销',
  },

  // 模型元数据编辑器(Models & Routes / Discovery 复用)
  metadata: {
    field: {
      logical_model_name: '建议逻辑模型名',
      display_name: '显示名称',
      context_window: '上下文窗口',
      max_input_tokens: '最大输入 Token',
      max_output_tokens: '最大输出 Token',
      input_modalities: '输入模态',
      output_modalities: '输出模态',
      tools: 'Tools',
      thinking: 'Thinking',
      web_search: 'Web Search',
      structured_output: '结构化输出',
      streaming: '流式',
      usage: '用量',
    },
    feature: {
      unknown: '未知',
      supported: '支持',
      unsupported: '不支持',
    },
  },
} as const;
