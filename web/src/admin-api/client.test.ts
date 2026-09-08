import { describe, expect, it, vi } from 'vitest';
import { AdminApiError, AdminClient, assertAdminPath } from './client';

describe('AdminClient', () => {
  it('uses the latest key for JSON and exports and prevents caller overrides of the transport policy', async () => {
    let key = 'first-key';
    const fetchImpl = vi.fn<typeof fetch>().mockImplementation(async () => new Response('{}'));
    const client = new AdminClient({ fetchImpl, getAdminKey: () => key });
    await client.json('/admin/sources');
    key = 'second-key';
    const controller = new AbortController();
    await client.blob('/admin/usage/export?format=csv', {
      signal: controller.signal, redirect: 'follow', cache: 'force-cache',
      headers: { Authorization: 'Bearer caller-key', Accept: 'text/csv' },
    });
    key = '';
    await client.json('/admin/sources', { headers: { Authorization: 'Bearer stale-key' } });
    const requests = fetchImpl.mock.calls.map(([, init]) => init!);
    expect(requests.map(init => new Headers(init.headers).get('Authorization'))).toEqual(['Bearer first-key', 'Bearer second-key', null]);
    expect(requests[1]).toMatchObject({ signal: controller.signal, cache: 'no-store', redirect: 'error' });
    expect(new Headers(requests[1].headers).get('Accept')).toBe('text/csv');
  });

  it('applies the same path and error policy to blob exports', async () => {
    const fetchImpl = vi.fn<typeof fetch>().mockResolvedValue(new Response('private response body', { status: 503 }));
    const client = new AdminClient({ fetchImpl, getAdminKey: () => 'secret' });
    await expect(client.blob('https://example.com/admin/export')).rejects.toThrow('Blocked non-Admin API endpoint');
    expect(fetchImpl).not.toHaveBeenCalled();
    await expect(client.blob('/admin/usage/export')).rejects.toMatchObject({ status: 503, code: 'admin_request_failed', message: 'Admin API request failed (503)' });
  });

  it('preserves cancellation and accepts empty mutation responses', async () => {
    const fetchImpl = vi.fn<typeof fetch>().mockRejectedValueOnce(new DOMException('Aborted', 'AbortError')).mockResolvedValueOnce(new Response(null, { status: 204 }));
    const client = new AdminClient({ fetchImpl });
    await expect(client.json('/admin/sources')).rejects.toMatchObject({ name: 'AbortError' });
    await expect(client.json('/admin/sources/source-a', { method: 'DELETE' })).resolves.toBeUndefined();
  });

  it('accepts only local /admin/ paths', () => {
    expect(assertAdminPath('/admin/capabilities?view=effective')).toBe('/admin/capabilities?view=effective');
    for (const path of [
      '/api/v1/usage',
      '/admin',
      '//example.com/admin/sources',
      'https://example.com/admin/sources',
      '/admin/../api/v1',
      '/admin/%2e%2e/api/v1',
      '/admin\\sources',
      '/admin/sources#secret',
    ]) {
      expect(() => assertAdminPath(path)).toThrow('Blocked non-Admin API endpoint');
    }
  });

  it('injects the Admin key only into Authorization and forwards AbortSignal', async () => {
    const controller = new AbortController();
    const fetchImpl = vi.fn(async () => new Response(JSON.stringify({ data: [] }), { status: 200 }));
    const client = new AdminClient({
      fetchImpl: fetchImpl as typeof fetch,
      getAdminKey: () => 'session-secret',
    });

    await expect(client.json('/admin/sources?status=enabled', { signal: controller.signal })).resolves.toEqual({ data: [] });
    const [url, init] = fetchImpl.mock.calls[0] as unknown as [string, RequestInit];
    const headers = new Headers(init.headers);
    expect(url).toBe('/admin/sources?status=enabled');
    expect(url).not.toContain('session-secret');
    expect(headers.get('Authorization')).toBe('Bearer session-secret');
    expect(init.signal).toBe(controller.signal);
    expect(init.cache).toBe('no-store');
  });

  it('returns structured errors without echoing non-JSON bodies', async () => {
    const structured = new AdminClient({
      fetchImpl: vi.fn(async () => new Response(JSON.stringify({
        error: { code: 'admin_unauthorized', message: 'Admin authentication required' },
      }), { status: 401 })) as typeof fetch,
    });
    await expect(structured.json('/admin/sources')).rejects.toMatchObject({
      name: 'AdminApiError',
      status: 401,
      code: 'admin_unauthorized',
      message: 'Admin authentication required',
    });

    const opaque = new AdminClient({
      fetchImpl: vi.fn(async () => new Response('internal-token=do-not-echo', { status: 502 })) as typeof fetch,
    });
    const error = await opaque.json('/admin/sources').catch((value: unknown) => value);
    expect(error).toBeInstanceOf(AdminApiError);
    expect(String(error)).toContain('Admin API request failed (502)');
    expect(String(error)).not.toContain('do-not-echo');
  });

  it('rejects successful non-JSON responses', async () => {
    const client = new AdminClient({
      fetchImpl: vi.fn(async () => new Response('<html>unexpected</html>', { status: 200 })) as typeof fetch,
    });
    await expect(client.json('/admin/sources')).rejects.toMatchObject({ code: 'invalid_json' });
  });
});
