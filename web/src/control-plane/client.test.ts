import { describe, expect, it, vi } from 'vitest';
import { assertAdminPath, ControlPlaneApiError, ControlPlaneClient } from './client';

describe('ControlPlaneClient', () => {
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
    const client = new ControlPlaneClient({
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
    const structured = new ControlPlaneClient({
      fetchImpl: vi.fn(async () => new Response(JSON.stringify({
        error: { code: 'admin_unauthorized', message: 'Admin authentication required' },
      }), { status: 401 })) as typeof fetch,
    });
    await expect(structured.json('/admin/sources')).rejects.toMatchObject({
      name: 'ControlPlaneApiError',
      status: 401,
      code: 'admin_unauthorized',
      message: 'Admin authentication required',
    });

    const opaque = new ControlPlaneClient({
      fetchImpl: vi.fn(async () => new Response('internal-token=do-not-echo', { status: 502 })) as typeof fetch,
    });
    const error = await opaque.json('/admin/sources').catch((value: unknown) => value);
    expect(error).toBeInstanceOf(ControlPlaneApiError);
    expect(String(error)).toContain('Admin API request failed (502)');
    expect(String(error)).not.toContain('do-not-echo');
  });

  it('rejects successful non-JSON responses', async () => {
    const client = new ControlPlaneClient({
      fetchImpl: vi.fn(async () => new Response('<html>unexpected</html>', { status: 200 })) as typeof fetch,
    });
    await expect(client.json('/admin/sources')).rejects.toMatchObject({ code: 'invalid_json' });
  });
});
