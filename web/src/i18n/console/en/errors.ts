// 接口/后端错误本地化(英文)——命名空间 console.errors
// 前端把 client 抛错 {status, code} 映射为当前语言文案,不再直接上屏后端 message。
export const errors = {
  admin_key_invalid: 'Admin Key could not be verified',
  service_unavailable: 'Service temporarily unavailable. Please try again later.',
  request_aborted: 'The request was aborted.',
  invalid_json: 'The service returned an invalid response.',
  admin_api_failed: 'Admin API request failed',
  usage_api_failed: 'Usage API request failed',
  export_failed: 'Usage export failed',
  http: 'Request failed (HTTP {{status}}).',
  validation_failed: 'The request was rejected: {{message}}',
  unknown: 'An unexpected error occurred.',
} as const;
