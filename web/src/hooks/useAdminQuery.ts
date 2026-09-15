import { normalizeAdminError } from '@/admin-api';
import { useQuerySession, type QuerySessionOptions } from './useQuerySession';

export function useAdminQuery<T>(options: QuerySessionOptions<T>) {
  const query = useQuerySession(options);
  return { ...query, error: query.error === undefined ? undefined : normalizeAdminError(query.error) };
}
