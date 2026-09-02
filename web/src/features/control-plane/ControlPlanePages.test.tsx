// @vitest-environment happy-dom
import { act } from 'react';
import { createRoot } from 'react-dom/client';
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';
import { setTestLanguage } from '@/test/setup';
import type { GatewayManagementPage as PageId } from '@/lib/consoleNavigation';
import { GatewayManagementPage } from '@/pages/GatewayManagementPage';

const jsonResponse = (value: unknown, status = 200) => new Response(JSON.stringify(value), {
  status,
  headers: { 'Content-Type': 'application/json' },
});

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
  auth_config: {},
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
  if (url === '/admin/provider-presets') return jsonResponse({ data: [] });
  if (url === '/admin/capabilities') return jsonResponse(capabilityResponse);
  if (url === '/admin/logical-models') return jsonResponse({ data: [logicalModel] });
  if (url === '/admin/model-bindings') return jsonResponse({ data: [binding] });
  if (url === '/admin/routes') return jsonResponse({ data: [route] });
  if (url.startsWith('/admin/sources/source-a/models')) return jsonResponse({ data: [sourceModel] });
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

describe('production control-plane pages', () => {
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

  const renderPage = async (page: PageId, options: { adminKeyConfigured?: boolean } = {}) => {
    await act(async () => {
      root.render(
        <GatewayManagementPage
          page={page}
          getAdminKey={() => ''}
          adminKeyConfigured={options.adminKeyConfigured ?? false}
          clearAdminKey={() => {}}
          refreshRevision={0}
          onLoadingChange={() => {}}
        />,
      );
      await new Promise((resolve) => setTimeout(resolve, 20));
    });
  };

  it('renders Source facts and never renders the credential environment reference', async () => {
    await renderPage('sources');

    expect(container.textContent).toContain('Source A');
    expect(container.textContent).toContain('原生');
    expect(container.textContent).toContain('转换');
    expect(container.textContent).toContain('不支持');

    const accountsTab = Array.from(container.querySelectorAll<HTMLButtonElement>('[role="tab"]'))
      .find((button) => button.textContent?.includes('账号'));
    await act(async () => accountsTab?.click());
    expect(container.textContent).toContain('Account A');
    expect(container.textContent).toContain('已配置');
    expect(container.textContent).not.toContain('PROVIDER_REFERENCE_ENV');
  });

  it('shows unsupported discovery and pending SourceModel field provenance', async () => {
    await renderPage('model-discovery');

    expect(container.textContent).toContain('unsupported');
    expect(container.textContent).toContain('discovery_unsupported');
    expect(container.textContent).toContain('upstream-a');
    expect(container.textContent).toContain('预设 1');
    expect(container.textContent).toContain('上游 1');
    expect(container.textContent).toContain('未知 1');
  });

  it('renders fixed three-protocol runtime facts without inferring unroutable as supported', async () => {
    await renderPage('capabilities');

    expect(container.textContent).toContain('Chat Completions');
    expect(container.textContent).toContain('Responses');
    expect(container.textContent).toContain('Messages');
    expect(container.textContent).toContain('primary #0');
    expect(container.textContent).toContain('fallback #1');
    expect(container.textContent).toContain('降级');
    expect(container.textContent).toContain('route_not_found');
  });

  it('keeps LogicalModel, ModelBinding, and Route as separate CRUD views', async () => {
    await renderPage('models-routes');
    expect(container.textContent).toContain('Model A');

    const tabs = () => Array.from(container.querySelectorAll<HTMLButtonElement>('[role="tab"]'));
    await act(async () => tabs().find((button) => button.textContent?.includes('绑定'))?.click());
    expect(container.textContent).toContain('upstream-a');

    await act(async () => tabs().find((button) => button.textContent?.includes('路由规则'))?.click());
    expect(container.textContent).toContain('固定主选 → 加权回退');
    expect(container.textContent).not.toMatch(/Random|Round-Robin/);
  });

  it('shows and copies a newly created recoverable Virtual Key', async () => {
    const oneTimeValue = ['one', 'time', 'value'].join('-');
    const writeText = vi.fn(async () => {});
    Object.defineProperty(navigator, 'clipboard', { configurable: true, value: { writeText } });
    vi.stubGlobal('fetch', vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
      const url = String(input);
      if (url === '/admin/keys' && init?.method === 'POST') {
        return jsonResponse({ id: 5, key: oneTimeValue, name: 'editor', allowed_models: [] }, 201);
      }
      return baseHandler(input);
    }));
    await renderPage('settings', { adminKeyConfigured: true });

    const newKey = Array.from(container.querySelectorAll<HTMLButtonElement>('button'))
      .find((button) => button.textContent?.includes('新建密钥'));
    await act(async () => {
      newKey?.click();
      await new Promise((resolve) => setTimeout(resolve, 0));
    });
    const input = document.body.querySelector<HTMLInputElement>('#virtual-key-editor-form input')!;
    await act(async () => {
      const setter = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')?.set;
      setter?.call(input, 'editor');
      input.dispatchEvent(new Event('input', { bubbles: true }));
    });
    const create = Array.from(document.body.querySelectorAll<HTMLButtonElement>('button'))
      .find((button) => button.textContent?.includes('创建密钥'));
    await act(async () => {
      create?.click();
      await new Promise((resolve) => setTimeout(resolve, 20));
    });

    expect(writeText).toHaveBeenCalledWith(oneTimeValue);
    expect(document.body.textContent).toContain(oneTimeValue);
    expect(document.body.textContent).toContain('API Key 已复制到剪贴板');
  });

  it('reveals an existing recoverable Virtual Key through the explicit Admin endpoint', async () => {
    const storedValue = ['stored', 'virtual', 'key'].join('-');
    vi.stubGlobal('fetch', vi.fn(async (input: RequestInfo | URL) => {
      const url = String(input);
      if (url === '/admin/keys') return jsonResponse({ data: [{
        id: 9,
        name: 'personal-app',
        key_prefix: 'gw_example',
        key_recoverable: true,
        allowed_models: [],
        enabled: true,
        created_at: '2026-09-01T00:00:00Z',
        last_used_at: null,
        revoked_at: null,
      }] });
      if (url === '/admin/keys/9/value') return jsonResponse({ data: { id: 9, key: storedValue } });
      return baseHandler(input);
    }));
    await renderPage('settings', { adminKeyConfigured: true });

    const reveal = Array.from(container.querySelectorAll<HTMLButtonElement>('button'))
      .find((button) => button.getAttribute('aria-label')?.includes('查看 personal-app 的 API Key'));
    await act(async () => {
      reveal?.click();
      await new Promise((resolve) => setTimeout(resolve, 20));
    });

    expect(document.body.textContent).toContain(storedValue);
  });

  it('renders structured 401 state with a retry action', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => jsonResponse({
      error: { code: 'unauthorized', message: 'admin key required' },
    }, 401)));
    await renderPage('capabilities');

    expect(container.textContent).toContain('Admin Key 未通过验证');
    expect(container.textContent).toContain('unauthorized');
    expect(container.textContent).toContain('重试');
  });
});
