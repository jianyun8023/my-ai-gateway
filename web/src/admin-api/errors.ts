import type { AdminErrorShape } from './types';

export const isAbortError = (error: unknown): boolean => (
  error instanceof Error && error.name === 'AbortError'
) || (typeof DOMException !== 'undefined' && error instanceof DOMException && error.name === 'AbortError');

export const normalizeAdminError = (error: unknown): AdminErrorShape => {
  if (isAbortError(error)) return { code: 'request_aborted', message: 'Request aborted' };
  if (error && typeof error === 'object') {
    const candidate = error as { status?: unknown; code?: unknown; message?: unknown };
    return {
      status: typeof candidate.status === 'number' ? candidate.status : undefined,
      code: typeof candidate.code === 'string' ? candidate.code : undefined,
      message: typeof candidate.message === 'string' ? candidate.message : 'Admin API request failed',
    };
  }
  return { code: 'unknown_error', message: 'Admin API request failed' };
};
