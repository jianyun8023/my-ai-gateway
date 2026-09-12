// @vitest-environment happy-dom
import type { ConsoleRoute, GatewayManagementPage as PageId, SourceSection } from '@/lib/consoleNavigation';
import { GatewayManagementPage } from '@/pages/GatewayManagementPage';
import { SourceForm } from '@/features/control-plane/sources/SourceForm';
import { Toggle } from '@/features/control-plane/shared';
import { FormActions } from '@/components/ui/FormActions';
import * as operationNotifications from '@/components/ui/notifications';
import type { Source } from '@/admin-api';
import { setTestLanguage } from '@/test/setup';
import { selectComboboxValue } from '@/test/interactions';
import { act } from 'react';
import { createRoot } from '@/test/render';
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';

const jsonResponse = (value: unknown, status = 200) => new Response(JSON.stringify(value), {
  status,
  headers: { 'Content-Type': 'application/json' },
});

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (error: Error) => void;
  const promise = new Promise<T>((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}

async function waitFor(condition: () => boolean) {
  const deadline = Date.now() + 1000;
  while (!condition() && Date.now() < deadline) {
    await act(async () => { await new Promise((resolve) => setTimeout(resolve, 20)); });
  }
  expect(condition()).toBe(true);
}

const source = {
  id: 'source-a',
  display_name: 'Source A',
  provider_preset_id: 'preset-a',
  provider_preset_version: 1,
  provider_preset_snapshot: {
    schema_version: 1,
    default_base_url: 'https://provider.example',
    protocols: {
      openai_chat_completions: { endpoint: '/chat', mode: 'native' },
      openai_responses: { endpoint: '/responses', mode: 'adapter' },
      anthropic_messages: { endpoint: '/messages', mode: 'unsupported' },
    },
    discovery: { support: 'unsupported', reason: 'No authenticated catalog endpoint' },
  },
  base_url: 'https://provider.example',
  endpoints: {
    openai_chat_completions: '/chat',
    openai_responses: '/responses',
    anthropic_messages: '/messages',
  },
  auth_config: { credential_header: { header: 'authorization', prefix: 'Bearer' } },
  protocol_capabilities: {
    openai_chat_completions: { mode: 'native' },
    openai_responses: { mode: 'adapter', source_protocol: 'anthropic_messages', adapter: 'adapter-a' },
    anthropic_messages: { mode: 'unsupported' },
  },
  enabled: true,
  created_at: '2026-08-31T00:00:00Z',
  updated_at: '2026-08-31T00:00:00Z',
};

const account = {
  id: 'account-a',
  source_id: 'source-a',
  display_name: 'Account A',
  credential_env: 'PROVIDER_REFERENCE_ENV',
  credential_configured: true,
  enabled: true,
  weight: 100,
  health_status: 'unknown',
  cooldown_until: null,
  created_at: '2026-08-31T00:00:00Z',
  updated_at: '2026-08-31T00:00:00Z',
};

const capabilityResponse = {
  version: 'v1',
  fact_source: 'runtime_snapshot',
  snapshot_revision: 7,
  snapshot_generated_at: '2026-08-31T00:00:00Z',
  data: [{
    route_id: 'route-a',
    source: { source_id: 'source-a', display_name: 'Source A' },
    account: { account_id: 'account-a', display_name: 'Account A', enabled: true },
    model: 'model-public',
    model_display_name: 'Model A',
    upstream_model_id: 'upstream-a',
    protocols: [{
      protocol_in: 'openai_chat_completions',
      status: 'routable',
      binding_id: 1,
      selection: 'primary',
      selection_rank: 0,
      protocol_upstream: 'openai_chat_completions',
      endpoint: 'https://provider.example/chat',
      mode: 'native',
      adapter: null,
      conversion_chain: [{ protocol_from: 'openai_chat_completions', protocol_to: 'openai_chat_completions', mode: 'native', adapter: null }],
      effective_capabilities: { streaming: 'native', tools: 'native', tool_streaming: 'unsupported', thinking: 'unsupported', web_search: 'unsupported', file_search: 'unsupported', vision: 'unsupported', usage: 'native' },
      degraded: false,
      degraded_features: [],
      allow_lossy_conversion: false,
      error: null,
    }, {
      protocol_in: 'openai_responses',
      status: 'routable',
      binding_id: 1,
      selection: 'fallback',
      selection_rank: 1,
      protocol_upstream: 'anthropic_messages',
      endpoint: 'https://provider.example/messages',
      mode: 'adapter',
      adapter: 'adapter-a',
      conversion_chain: [{ protocol_from: 'openai_responses', protocol_to: 'anthropic_messages', mode: 'adapter', adapter: 'adapter-a' }],
      effective_capabilities: { streaming: 'translated', tools: 'translated', tool_streaming: 'unsupported', thinking: 'translated', web_search: 'unsupported', file_search: 'unsupported', vision: 'unsupported', usage: 'translated' },
      degraded: true,
      degraded_features: ['streaming', 'tools', 'thinking', 'usage'],
      allow_lossy_conversion: true,
      error: null,
    }, {
      protocol_in: 'anthropic_messages',
      status: 'unroutable',
      binding_id: null,
      selection: null,
      selection_rank: null,
      protocol_upstream: null,
      endpoint: null,
      mode: null,
      adapter: null,
      conversion_chain: [],
      effective_capabilities: { streaming: 'unsupported', tools: 'unsupported', tool_streaming: 'unsupported', thinking: 'unsupported', web_search: 'unsupported', file_search: 'unsupported', vision: 'unsupported', usage: 'unsupported' },
      degraded: false,
      degraded_features: [],
      allow_lossy_conversion: null,
      error: { code: 'route_not_found', message: 'No published route', route_id: 'route-a' },
    }],
  }],
};

const logicalModel = {
  id: 'logical-a',
  public_name: 'model-public',
  display_name: 'Model A',
  status: 'confirmed',
  metadata: {},
  field_sources: {},
  enabled: true,
  request_timeout_ms: null,
  max_retries: null,
  confirmed_at: '2026-08-31T00:00:00Z',
  unavailable_at: null,
  created_at: '2026-08-31T00:00:00Z',
  updated_at: '2026-08-31T00:00:00Z',
};

const binding = {
  id: 1,
  logical_model_id: 'logical-a',
  source_id: 'source-a',
  account_id: 'account-a',
  upstream_model_id: 'upstream-a',
  protocol: 'openai_chat_completions',
  status: 'confirmed',
  enabled: true,
  priority: 100,
  confirmed_at: '2026-08-31T00:00:00Z',
  unavailable_at: null,
  created_at: '2026-08-31T00:00:00Z',
  updated_at: '2026-08-31T00:00:00Z',
};

const route = {
  id: 'route-a',
  logical_model_id: 'logical-a',
  public_name: 'model-public',
  protocols: ['openai_chat_completions'],
  strategy: 'primary_then_weighted_fallback',
  allow_lossy_conversion: false,
  enabled: true,
  created_at: '2026-08-31T00:00:00Z',
  updated_at: '2026-08-31T00:00:00Z',
};

const sourceModel = {
  source_id: 'source-a',
  upstream_model_id: 'upstream-a',
  confirmation_status: 'pending',
  availability_status: 'available',
  raw_snapshot: {},
  metadata: { display_name: 'Upstream A', context_window: 128000, tools: 'unknown' },
  field_sources: { display_name: 'preset', context_window: 'upstream', tools: 'unknown' },
  matched_model_preset_id: 'model-preset-a',
  matched_model_preset_version: 1,
  first_discovered_at: '2026-08-31T00:00:00Z',
  last_discovered_at: '2026-08-31T00:00:00Z',
  confirmed_at: null,
  unavailable_at: null,
  created_at: '2026-08-31T00:00:00Z',
  updated_at: '2026-08-31T00:00:00Z',
};

const baseHandler = async (input: RequestInfo | URL) => {
  const url = String(input);
  if (url === '/admin/sources') return jsonResponse({ data: [source] });
  if (url === '/admin/accounts') return jsonResponse({ data: [account] });
  if (url === '/admin/provider-presets') return jsonResponse({ data: [{ id: 'preset-a', version: 1, display_name: 'Synthetic preset', definition: source.provider_preset_snapshot, created_at: source.created_at }] });
  if (url === '/admin/capabilities') return jsonResponse(capabilityResponse);
  if (url === '/admin/logical-models') return jsonResponse({ data: [logicalModel] });
  if (url === '/admin/logical-models/logical-a/routing') return jsonResponse({ data: {
    logical_model: logicalModel,
    lines: [{ source_id: 'source-a', account_id: 'account-a', upstream_model_id: 'upstream-a', protocols: ['openai_chat_completions'] }],
    protocols: ['openai_chat_completions'], strategy: 'primary_then_weighted_fallback',
    request_timeout_ms: null, max_retries: null,
  } });
  if (url === '/admin/sources/source-a/models/upstream-a/capabilities') return jsonResponse({ data: [{
    source_id: 'source-a', upstream_model_id: 'upstream-a', protocol: 'openai_chat_completions',
    status: 'confirmed', mode: 'native', feature_capabilities: {}, field_source: 'user',
  }] });
  if (url === '/admin/model-bindings') return jsonResponse({ data: [binding] });
  if (url === '/admin/routes') return jsonResponse({ data: [route] });
  if (url.startsWith('/admin/sources/source-a/models')) return jsonResponse({ data: [url.includes('confirmation_status=confirmed') ? { ...sourceModel, confirmation_status: 'confirmed' } : sourceModel] });
  if (url === '/admin/sources/source-a/preset-diff') return jsonResponse({ data: {
    source_id: 'source-a', provider_preset_id: 'preset-a', source_version: 1, latest_version: 1, changes: [],
  } });
  if (url.startsWith('/admin/events?')) return jsonResponse({ version: 'v1', timezone: 'UTC', fact_source: 'postgresql_unified_read_model', data: [], page: { limit: 6, has_more: false } });
  if (url === '/admin/sources/source-a/discoveries/latest') return jsonResponse({
    data: {
      id: 3,
      source_id: 'source-a',
      account_id: 'account-a',
      provider_preset_id: 'preset-a',
      provider_preset_version: 1,
      status: 'unsupported',
      raw_snapshot: null,
      diff: { added: [], changed: [], missing: [] },
      discovered_model_count: 0,
      http_status: null,
      latency_ms: 1,
      error_code: 'discovery_unsupported',
      error_message: 'No authenticated catalog endpoint',
      requested_by: 'admin-ui',
      started_at: '2026-08-31T00:00:00Z',
      completed_at: '2026-08-31T00:00:00Z',
    },
    diff: { added: [], changed: [], missing: [] },
    last_discovered_at: '2026-08-31T00:00:00Z',
  });
  if (url === '/admin/keys') return jsonResponse({ data: [] });
  return jsonResponse({ error: { code: 'not_found', message: `Unhandled ${url}` } }, 404);
};

describe('PR #200 audit reproductions', () => {
  let container: HTMLDivElement;

  beforeAll(() => setTestLanguage('zh'));
  let root: ReturnType<typeof createRoot>;

  beforeEach(() => {
    globalThis.IS_REACT_ACT_ENVIRONMENT = true;
    container = document.createElement('div');
    document.body.appendChild(container);
    root = createRoot(container);
    vi.stubGlobal('fetch', vi.fn(baseHandler));
  });

  afterEach(() => {
    act(() => root.unmount());
    container.remove();
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
  });

  const renderPage = async (
    page: PageId,
    options: {
      adminKeyConfigured?: boolean;
      route?: ConsoleRoute;
      onOpenSource?: (sourceId: string, section?: SourceSection) => void;
      refreshRevision?: number;
    } = {},
  ) => {
    await act(async () => {
      root.render(
        <GatewayManagementPage
          page={page}
          route={options.route}
          onOpenSource={options.onOpenSource}
          getAdminKey={() => ''}
          adminKeyConfigured={options.adminKeyConfigured ?? false}
          clearAdminKey={() => {}}
          refreshRevision={options.refreshRevision ?? 0}
          onLoadingChange={() => {}}
        />,
      );
      await new Promise((resolve) => setTimeout(resolve, 20));
    });
  };

  const renderReview = (options: Parameters<typeof renderPage>[1] = {}) => (
    renderPage('sources', { ...options, route: { page: 'sources', sourceId: 'source-a', section: 'review' } })
  );

  it('search select-all must not select a hidden pending model', async () => {
    vi.stubGlobal('fetch', vi.fn(async (input: RequestInfo | URL) => {
      if (String(input) === '/admin/sources/source-a/models') {
        return jsonResponse({ data: [sourceModel, { ...sourceModel, upstream_model_id: 'upstream-b', metadata: {} }] });
      }
      return baseHandler(input);
    }));
    await renderReview();
    const fieldId = [...container.querySelectorAll('label')].find(label => label.textContent === '搜索模型')!.htmlFor;
    const field = document.getElementById(fieldId) as HTMLInputElement;
    await act(async () => {
      Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')!.set!.call(field, 'upstream-a');
      field.dispatchEvent(new Event('input', { bubbles: true }));
    });
    expect(container.querySelector('input[aria-label="选择 upstream-b"]')).toBeNull();
    await act(async () => container.querySelector<HTMLInputElement>('input[aria-label="选择全部可确认模型"]')!.click());
    const selectedLabel = [...container.querySelectorAll('button')].find(button => button.textContent?.includes('批量确认'))!.textContent;
    expect(selectedLabel).toBe('批量确认 (1)');
  });

  it.each(['failed', 'unsupported'])('batch checks must not count a %s execution as success', async (status) => {
    const successNotice = vi.spyOn(operationNotifications, 'notifySuccess');
    vi.stubGlobal('fetch', vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
      if (String(input).endsWith('/discoveries') && init?.method === 'POST') {
        return jsonResponse({ data: { run: { status, id: 10, discovered_model_count: 0, error_code: 'audit_execution_' + status }, diff: { added: [], changed: [], missing: [] }, models: [] } });
      }
      return baseHandler(input);
    }));
    await renderPage('sources');
    await act(async () => [...container.querySelectorAll('button')].find(button => button.textContent === '批量检查更新')!.click());
    expect(successNotice).toHaveBeenCalledTimes(1);
    expect(successNotice.mock.calls[0][0]).not.toContain('成功 1 个');
  });

  it('all-protocol connection testing must surface HTTP failures', async () => {
    vi.stubGlobal('fetch', vi.fn(async (input: RequestInfo | URL) => {
      if (String(input).endsWith('/connection-tests')) {
        return jsonResponse({ error: { code: 'audit_service_unavailable', message: 'Connection test service unavailable' } }, 503);
      }
      return baseHandler(input);
    }));
    await renderPage('sources', { route: { page: 'sources', sourceId: 'source-a' } });
    await act(async () => [...container.querySelectorAll('button')].find(button => button.textContent === '测试连接')!.click());
    expect(container.querySelector('[role="alert"]')).not.toBeNull();
  });

  it('detail check-updates must prevent duplicate in-flight submissions', async () => {
    const pending = deferred<void>();
    let requests = 0;
    vi.stubGlobal('fetch', vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
      if (String(input).endsWith('/discoveries') && init?.method === 'POST') {
        requests += 1;
        await pending.promise;
        return jsonResponse({ data: { run: { status: 'succeeded', id: 10, discovered_model_count: 1 }, diff: { added: [], changed: [], missing: [] }, models: [] } });
      }
      return baseHandler(input);
    }));
    await renderPage('sources', { route: { page: 'sources', sourceId: 'source-a' }, onOpenSource: vi.fn() });
    const button = [...container.querySelectorAll('button')].find(button => button.textContent === '检查模型更新')!;
    try {
      await act(async () => button.click());
      await act(async () => button.click());
      expect(requests).toBe(1);
    } finally {
      await act(async () => pending.resolve(undefined));
    }
  });
});
