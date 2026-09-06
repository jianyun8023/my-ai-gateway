import { useCallback } from 'react';
import { useTranslation } from 'react-i18next';

/** 从任意 client 抛错中提取可用于映射的 {status, code},不读取后端 message。 */
const extractErrorMeta = (error: unknown): { status?: number; code?: string; message?: string } => {
  if (error && typeof error === 'object') {
    const candidate = error as { status?: unknown; code?: unknown; message?: unknown };
    return {
      status: typeof candidate.status === 'number' ? candidate.status : undefined,
      code: typeof candidate.code === 'string' ? candidate.code : undefined,
      message: typeof candidate.message === 'string' && candidate.message.trim() !== ''
        ? candidate.message
        : undefined,
    };
  }
  return {};
};

/**
 * 把接口/后端错误映射为当前界面语言的文案。
 * 校验类错误(HTTP 422 / validation_failed)的后端 message 携带可执行的修复线索
 * (例如缺少已确认能力),随本地化前缀一起上屏;其余后端 message 仍只留在控制台日志。
 */
export const useLocalizedApiError = () => {
  const { t } = useTranslation('console');
  return useCallback((error: unknown): string => {
    const { status, code, message } = extractErrorMeta(error);
    if (status === 401 || code === 'unauthorized') {
      return t('errors.admin_key_invalid');
    }
    if (code === 'request_aborted') return t('errors.request_aborted');
    if (code === 'invalid_json') return t('errors.invalid_json');
    if (status !== undefined && status >= 500) return t('errors.service_unavailable');
    if (status === 422 || code === 'validation_failed') {
      return message ? t('errors.validation_failed', { message }) : t('errors.http', { status });
    }
    if (status !== undefined) return t('errors.http', { status });
    if (code === 'admin_request_failed') return t('errors.admin_api_failed');
    if (code === 'unknown_error') return t('errors.unknown');
    return t('errors.unknown');
  }, [t]);
};
