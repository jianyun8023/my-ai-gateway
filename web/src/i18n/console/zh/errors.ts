// 接口/后端错误本地化(简体中文)——命名空间 console.errors
export const errors = {
  admin_key_invalid: 'Admin Key 未通过验证',
  service_unavailable: '服务暂时不可用,请稍后重试。',
  request_aborted: '请求已取消。',
  invalid_json: '服务返回了无法解析的响应。',
  admin_api_failed: 'Admin API 请求失败',
  usage_api_failed: '用量 API 请求失败',
  export_failed: '用量导出失败',
  http: '请求失败(HTTP {{status}})。',
  unknown: '发生未知错误。',
} as const;
