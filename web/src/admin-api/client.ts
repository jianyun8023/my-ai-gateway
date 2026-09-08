export interface AdminClientOptions {
  fetchImpl?: typeof fetch;
  getAdminKey?: () => string;
}

interface AdminErrorEnvelope {
  error?: {
    code?: unknown;
    message?: unknown;
  };
}

export class AdminApiError extends Error {
  constructor(
    message: string,
    readonly status: number,
    readonly code: string,
  ) {
    super(message);
    this.name = 'AdminApiError';
  }
}

export interface AdminTransport {
  json<T>(path: string, init?: RequestInit): Promise<T>;
  blob(path: string, init?: RequestInit): Promise<Blob>;
}

const ADMIN_ORIGIN = 'http://gateway.invalid';

export const assertAdminPath = (path: string): string => {
  if (
    !path.startsWith('/admin/')
    || path.startsWith('//')
    || path.includes('\\')
    || path.includes('#')
  ) {
    throw new Error('Blocked non-Admin API endpoint');
  }

  const parsed = new URL(path, ADMIN_ORIGIN);
  if (parsed.origin !== ADMIN_ORIGIN || !parsed.pathname.startsWith('/admin/')) {
    throw new Error('Blocked non-Admin API endpoint');
  }
  return `${parsed.pathname}${parsed.search}`;
};

const parseJson = (text: string): unknown => {
  try {
    return JSON.parse(text) as unknown;
  } catch {
    return undefined;
  }
};

const readError = (payload: unknown): { code?: string; message?: string } => {
  if (!payload || typeof payload !== 'object') return {};
  const error = (payload as AdminErrorEnvelope).error;
  if (!error || typeof error !== 'object') return {};
  return {
    code: typeof error.code === 'string' ? error.code : undefined,
    message: typeof error.message === 'string' ? error.message : undefined,
  };
};

export class AdminClient implements AdminTransport {
  private readonly fetchImpl: typeof fetch;
  private readonly getAdminKey: () => string;

  constructor(options: AdminClientOptions = {}) {
    this.fetchImpl = options.fetchImpl ?? (
      typeof window === 'undefined' ? fetch : window.fetch.bind(window)
    );
    this.getAdminKey = options.getAdminKey ?? (() => '');
  }

  private async request(path: string, init: RequestInit): Promise<Response> {
    const safePath = assertAdminPath(path);
    const headers = new Headers(init.headers);
    if (!headers.has('Accept')) headers.set('Accept', 'application/json');
    headers.delete('Authorization');
    const adminKey = this.getAdminKey().trim();
    if (adminKey) headers.set('Authorization', `Bearer ${adminKey}`);

    const response = await this.fetchImpl(safePath, {
      ...init,
      headers,
      cache: 'no-store',
      redirect: 'error',
    });
    if (!response.ok) {
      const error = readError(parseJson(await response.text()));
      throw new AdminApiError(
        error.message ?? `Admin API request failed (${response.status})`,
        response.status,
        error.code ?? 'admin_request_failed',
      );
    }

    return response;
  }

  async blob(path: string, init: RequestInit = {}): Promise<Blob> {
    return (await this.request(path, init)).blob();
  }

  async json<T>(path: string, init: RequestInit = {}): Promise<T> {
    const response = await this.request(path, init);
    const text = await response.text();
    const payload = text ? parseJson(text) : undefined;
    if (response.status === 204 || !text) return undefined as T;
    if (payload === undefined) {
      throw new AdminApiError('Admin API returned a non-JSON response', response.status, 'invalid_json');
    }
    return payload as T;
  }
}
