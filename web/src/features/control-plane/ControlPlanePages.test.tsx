// @vitest-environment happy-dom
import type { GatewayManagementPage as PageId } from '@/lib/consoleNavigation';
import { GatewayManagementPage } from '@/pages/GatewayManagementPage';
import { SourceForm } from './sources/SourceForm';
import { Toggle } from './shared';
import { FormActions } from '@/components/ui/FormActions';
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

  it('only selects available pending discoveries and clears selection after filtering', async () => {
    vi.stubGlobal('fetch', vi.fn(async (input: RequestInfo | URL) => {
      if (String(input).startsWith('/admin/sources/source-a/models')) {
        const models = [sourceModel, { ...sourceModel, upstream_model_id: 'unknown-model', availability_status: 'unknown' }];
        const availability = new URL(String(input), 'http://localhost').searchParams.get('availability_status');
        return jsonResponse({ data: models.filter(model => !availability || model.availability_status === availability) });
      }
      return baseHandler(input);
    }));
    await renderPage('discovery');
    expect(container.querySelector('section[aria-label="模型发现筛选"]')).not.toBeNull();
    const checkbox = (name: string) => container.querySelector<HTMLInputElement>(`input[aria-label="${name}"]`)!;
    expect(checkbox('选择 unknown-model').disabled).toBe(true);
    act(() => checkbox('选择全部可确认模型').click());
    expect(checkbox('选择 upstream-a').checked).toBe(true);
    expect(checkbox('选择 unknown-model').checked).toBe(false);
    const filterId = [...container.querySelectorAll('label')].find(label => label.textContent === '可用状态')!.htmlFor;
    const filter = document.getElementById(filterId) as HTMLInputElement;
    await selectComboboxValue(filter, 'unknown');
    expect(checkbox('选择 upstream-a')).toBeNull();
    expect(checkbox('选择全部可确认模型').checked).toBe(false);
    expect([...container.querySelectorAll('button')].find(button => button.textContent?.includes('批量确认'))!.disabled).toBe(true);
  });

  it('reports boolean toggle changes and ignores disabled activation', () => {
    const onChange = vi.fn();
    act(() => root.render(<Toggle label="来源启用" checked onChange={onChange} />));
    act(() => container.querySelector<HTMLInputElement>('input')!.click());
    expect(onChange).toHaveBeenCalledExactlyOnceWith(false);
    expect(container.querySelector('input')!.getAttribute('aria-label')).toBe('来源启用');
    act(() => root.render(<Toggle label="来源启用" checked={false} disabled onChange={onChange} />));
    act(() => container.querySelector<HTMLInputElement>('input')!.click());
    expect(onChange).toHaveBeenCalledTimes(1);
  });

  it('returns focus to the source row after switching from details to editing', async () => {
    await renderPage('sources');
    const trigger = container.querySelector<HTMLButtonElement>('button[aria-label="查看 source-a"]')!;
    await act(async () => { trigger.focus(); trigger.click(); });
    const edit = [...container.querySelectorAll<HTMLButtonElement>('[role="dialog"] button')].find(button => button.textContent === '编辑来源')!;
    act(() => { edit.focus(); edit.click(); });
    await act(async () => { await new Promise(resolve => setTimeout(resolve, 250)); });
    expect(container.querySelector('#source-editor-form')).not.toBeNull();
    const close = container.querySelector<HTMLButtonElement>('[role="dialog"] button[aria-label="关闭"]')!;
    act(() => { close.focus(); close.click(); });
    await act(async () => { await new Promise(resolve => setTimeout(resolve, 30)); });
    expect(document.activeElement).toBe(trigger);
  });

  it('keeps source table actions separate from row details and preserves column semantics', async () => {
    await renderPage('sources');
    const table = container.querySelector('table')!;
    expect([...table.querySelectorAll('thead th')].every(cell => cell.getAttribute('scope') === 'col')).toBe(true);
    const region = table.closest('[role="region"]')!;
    expect(region.getAttribute('aria-label')).toBeTruthy();
    expect(region.getAttribute('tabindex')).toBe('0');
    expect(table.querySelector('tbody tr')!.getAttribute('role')).toBeNull();

    // The edit button bubbles through its cell, but must not also open row details.
    await act(async () => container.querySelector<HTMLButtonElement>('button[aria-label="编辑 source-a"]')!.click());
    expect(container.querySelector('#source-editor-form')).not.toBeNull();
    expect(container.querySelectorAll('[role="dialog"]')).toHaveLength(1);
    act(() => container.querySelector<HTMLButtonElement>('[role="dialog"] button[aria-label="关闭"]')!.click());
    await act(async () => { await new Promise(resolve => setTimeout(resolve, 30)); });
    expect(container.querySelector('[role="dialog"]')).toBeNull();

    await act(async () => table.querySelector<HTMLTableCellElement>('tbody td')!.click());
    expect(container.querySelector('[role="dialog"]')).not.toBeNull();
    expect(container.querySelector('#source-editor-form')).toBeNull();
  });

  it('keeps source field validation, native selection, checkbox state and external submit behavior', async () => {
    const onSubmit = vi.fn();
    const record = { ...source, auth_config: { credential_header: { header: 'authorization', prefix: 'Bearer' } } } as Source;
    const presets = [{ id: 'preset-a', version: 1, display_name: 'Preset A', definition: record.provider_preset_snapshot, created_at: source.created_at }];
    const render = (busy = false) => root.render(<><SourceForm record={record} presets={presets} busy={busy} onSubmit={onSubmit} />
      <FormActions form="source-editor-form" cancelLabel="Cancel" submitLabel="Save source" busy={busy} onCancel={() => {}} /></>);
    act(() => render());
    const input = (label: string) => {
      const id = [...container.querySelectorAll('label')].find(element => element.textContent === label)!.htmlFor;
      return document.getElementById(id) as HTMLInputElement;
    };
    const name = input('显示名称');
    const setValue = (element: HTMLInputElement | HTMLTextAreaElement, value: string) => {
      const prototype = element.tagName === 'TEXTAREA' ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
      Object.getOwnPropertyDescriptor(prototype, 'value')!.set!.call(element, value);
      element.dispatchEvent(new Event('input', { bubbles: true }));
    };
    const headers = input('默认请求头(JSON)') as unknown as HTMLTextAreaElement;
    act(() => { setValue(name, 'Updated source'); setValue(headers, '{'); });
    const submit = container.querySelector<HTMLButtonElement>('button[form="source-editor-form"]')!;
    act(() => submit.click());
    expect(onSubmit).not.toHaveBeenCalled();
    expect(container.querySelector('[role="alert"]')).not.toBeNull();
    expect(name.value).toBe('Updated source');
    const mode = container.querySelector<HTMLInputElement>('input[role="combobox"]:not(:disabled)')!;
    act(() => setValue(headers, '{}'));
    await selectComboboxValue(mode, 'unsupported');
    act(() => container.querySelector<HTMLInputElement>('input[type="checkbox"]')!.click());
    act(() => submit.click());
    expect(onSubmit).toHaveBeenCalledExactlyOnceWith(expect.objectContaining({
      display_name: 'Updated source', enabled: false,
      protocol_capabilities: expect.objectContaining({ openai_chat_completions: expect.objectContaining({ mode: 'unsupported' }) }),
    }));
    act(() => render(true));
    expect(name.disabled).toBe(true);
    expect(mode.disabled).toBe(true);
    act(() => submit.click());
    expect(onSubmit).toHaveBeenCalledTimes(1);
  });

  it('shows unsupported discovery and pending SourceModel field provenance', async () => {
    await renderPage('discovery');

    expect(container.textContent).toContain('unsupported');
    expect(container.textContent).toContain('discovery_unsupported');
    expect(container.textContent).toContain('upstream-a');
    expect(container.textContent).toContain('预设 1');
    expect(container.textContent).toContain('上游 1');
    expect(container.textContent).toContain('未知 1');
    expect(container.querySelector('[role="status"]')?.textContent).toContain('discovery_unsupported');
  });

  it('preserves the source form after a failed save and emits only one success on retry', async () => {
    let failSave = true;
    const save = vi.fn(async (init: RequestInit) => failSave
      ? jsonResponse({ error: { code: 'source_conflict', message: 'Retry source save' } }, 409)
      : jsonResponse({ data: { ...source, ...JSON.parse(init.body as string) } }));
    vi.stubGlobal('fetch', vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
      if (String(input) === '/admin/sources/source-a' && init?.method === 'PUT') return save(init);
      return baseHandler(input);
    }));
    await renderPage('sources');
    await act(async () => container.querySelector<HTMLButtonElement>('button[aria-label="编辑 source-a"]')!.click());
    const form = container.querySelector('#source-editor-form')!;
    const name = [...form.querySelectorAll('input')].find(el => el.value === 'Source A')!;
    act(() => {
      Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')!.set!.call(name, 'Edited source');
      name.dispatchEvent(new Event('input', { bubbles: true }));
    });
    const submit = () => container.querySelector<HTMLButtonElement>('button[form="source-editor-form"]')!.click();
    await act(async () => { submit(); });
    expect(form.querySelector('[role="alert"]')).not.toBeNull();
    expect(name.value).toBe('Edited source');
    expect(container.querySelector('.mantine-Notification-root')).toBeNull();
    failSave = false;
    await act(async () => { submit(); });
    expect(save).toHaveBeenCalledTimes(2);
    expect(container.querySelectorAll('.mantine-Notification-root[role="status"]')).toHaveLength(1);
    expect(container.querySelector('[role="alert"]')).toBeNull();
    expect(container.querySelector('[role="dialog"]')).toBeNull();
  });

  it('retains discovery failure details alongside the existing model catalog', async () => {
    vi.stubGlobal('fetch', vi.fn(async (input: RequestInfo | URL) => {
      const response = await baseHandler(input);
      if (String(input) !== '/admin/sources/source-a/discoveries/latest') return response;
      const latest = await response.json();
      latest.data.status = 'failed';
      latest.data.error_code = 'upstream_timeout';
      latest.data.error_message = 'Catalog request timed out';
      return jsonResponse(latest);
    }));
    await renderPage('discovery');
    const alert = container.querySelector('[role="alert"]')!;
    expect(alert.textContent).toContain('upstream_timeout');
    expect(alert.textContent).toContain('Catalog request timed out');
    expect(container.querySelector('tbody')?.textContent).toContain('upstream-a');
  });

  it('does not present a failed initial discovery query as an empty catalog', async () => {
    vi.stubGlobal('fetch', vi.fn(async (input: RequestInfo | URL) => {
      if (String(input) === '/admin/sources/source-a/discoveries/latest') {
        return jsonResponse({ error: { code: 'catalog_unavailable', message: 'Synthetic catalog failure' } }, 503);
      }
      return baseHandler(input);
    }));
    await renderPage('discovery');
    expect(container.querySelector('[role="alert"]')?.textContent).toContain('catalog_unavailable');
    expect(container.textContent).not.toContain('当前筛选没有来源模型');
    expect(container.textContent).not.toContain('该来源不支持自动发现');
  });

  it('isolates a slow Source switch and ignores the old Source response even when fetch ignores abort', async () => {
    const sourceB = { ...source, id: 'source-b', display_name: 'Source B' };
    const accountB = { ...account, id: 'account-b', source_id: 'source-b', display_name: 'Account B' };
    const modelB = { ...sourceModel, source_id: 'source-b', upstream_model_id: 'upstream-b' };
    const oldLatest = deferred<Response>();
    const newLatest = deferred<Response>();
    vi.stubGlobal('fetch', vi.fn(async (input: RequestInfo | URL) => {
      const url = String(input);
      if (url === '/admin/sources') return jsonResponse({ data: [source, sourceB] });
      if (url === '/admin/accounts') return jsonResponse({ data: [account, accountB] });
      if (url === '/admin/sources/source-a/discoveries/latest') return oldLatest.promise;
      if (url === '/admin/sources/source-b/discoveries/latest') return newLatest.promise;
      if (url.startsWith('/admin/sources/source-b/models')) return jsonResponse({ data: [modelB] });
      return baseHandler(input);
    }));
    await renderPage('discovery');
    const sourceLabel = [...container.querySelectorAll('label')].find((label) => label.textContent === '来源')!;
    const sourceSelect = document.getElementById(sourceLabel.htmlFor) as HTMLInputElement;
    await selectComboboxValue(sourceSelect, 'source-b');
    expect(container.textContent).not.toContain('upstream-a');
    expect(container.textContent).not.toContain('当前筛选没有来源模型');
    expect(container.textContent).toContain('正在加载来源模型');
    const latestB = await (await baseHandler('/admin/sources/source-a/discoveries/latest')).json();
    latestB.data.source_id = 'source-b';
    latestB.data.account_id = 'account-b';
    await act(async () => newLatest.resolve(jsonResponse(latestB)));
    expect(container.querySelector('tbody')?.textContent).toContain('upstream-b');
    await act(async () => oldLatest.resolve(await baseHandler('/admin/sources/source-a/discoveries/latest')));
    expect(container.querySelector('tbody')?.textContent).toContain('upstream-b');
    expect(container.textContent).not.toContain('upstream-a');
  });

  it('keeps a confirmed no-Source state visible when its background refresh fails', async () => {
    let failRefresh = false;
    const fetchRequest = vi.fn(async (input: RequestInfo | URL) => {
      if (String(input) === '/admin/sources') {
        return failRefresh
          ? jsonResponse({ error: { code: 'source_refresh_failed', message: 'Synthetic Source refresh failure' } }, 503)
          : jsonResponse({ data: [] });
      }
      if (String(input) === '/admin/accounts') return jsonResponse({ data: [] });
      return baseHandler(input);
    });
    vi.stubGlobal('fetch', fetchRequest);
    const render = async (refreshRevision: number) => {
      await act(async () => {
        root.render(<GatewayManagementPage page="discovery" getAdminKey={() => ''} adminKeyConfigured={false}
          clearAdminKey={() => {}} refreshRevision={refreshRevision} onLoadingChange={() => {}} />);
        await new Promise((resolve) => setTimeout(resolve, 20));
      });
    };
    await render(0);
    expect(container.textContent).toContain('没有可用于模型发现的来源');
    failRefresh = true;
    await render(1);
    expect(container.querySelector('[role="alert"]')?.textContent).toContain('source_refresh_failed');
    expect(container.textContent).toContain('没有可用于模型发现的来源');
    expect([...container.querySelectorAll('button')].some((button) => button.textContent?.includes('重试'))).toBe(true);
  });

  it.each(['failed', 'unsupported'])('reports a %s discovery once with its code and existing catalog', async (status) => {
    const latest = await (await baseHandler('/admin/sources/source-a/discoveries/latest')).json();
    let ran = false;
    vi.stubGlobal('fetch', vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
      if (String(input).endsWith('/discoveries') && init?.method === 'POST') {
        ran = true;
        latest.data = { ...latest.data, status, error_code: 'catalog_unavailable', error_message: null };
        return jsonResponse({ data: { run: latest.data, diff: latest.diff } });
      }
      if (String(input).endsWith('/discoveries/latest') && ran) return jsonResponse(latest);
      return baseHandler(input);
    }));
    await renderPage('discovery');
    await act(async () => [...container.querySelectorAll('button')].find(el => el.textContent === '运行发现')!.click());
    expect(ran).toBe(true);
    expect(container.querySelectorAll(status === 'failed' ? '[role="alert"]' : '[role="status"]')).toHaveLength(1);
    expect(container.textContent?.split('catalog_unavailable')).toHaveLength(2);
    expect(container.querySelector('tbody')?.textContent).toContain('upstream-a');
    expect(container.querySelector('.mantine-Notification-root')).toBeNull();
  });

  it('keeps the catalog and retry when a completed discovery cannot refresh its result', async () => {
    let ran = false;
    let failRefresh = true;
    vi.stubGlobal('fetch', vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
      if (String(input).endsWith('/discoveries') && init?.method === 'POST') {
        ran = true;
        return jsonResponse({ data: { run: { status: 'succeeded', id: 4, discovered_model_count: 1 } } });
      }
      if (ran && failRefresh && String(input).endsWith('/discoveries/latest')) return jsonResponse({ error: { code: 'refresh_unavailable', message: 'Refresh failed' } }, 503);
      return baseHandler(input);
    }));
    await renderPage('discovery');
    await act(async () => [...container.querySelectorAll('button')].find(el => el.textContent === '运行发现')!.click());
    expect(container.querySelector('[role="alert"]')?.textContent).toContain('refresh_unavailable');
    expect(container.querySelector('tbody')?.textContent).toContain('upstream-a');
    expect(container.querySelector('.mantine-Notification-root')).toBeNull();
    failRefresh = false;
    await act(async () => container.querySelector<HTMLButtonElement>('[role="alert"] button')!.click());
    expect(container.querySelector('[role="alert"]')).toBeNull();
  });

  it('keeps capability drafts on failure and reports successful retry while the modal stays open', async () => {
    let failSave = true;
    vi.stubGlobal('fetch', vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
      const path = String(input);
      if (path.endsWith('/capabilities/openai_chat_completions')) {
        if (failSave) return jsonResponse({ error: { code: 'save_conflict', message: 'Retry capability save' } }, 409);
        return jsonResponse({ data: { ...JSON.parse(init!.body as string), protocol: 'openai_chat_completions' } });
      }
      if (path.endsWith('/models/upstream-a/capabilities')) return jsonResponse({ data: [] });
      return baseHandler(input);
    }));
    await renderPage('discovery');
    await act(async () => [...container.querySelectorAll('button')].find(el => el.textContent === '协议能力')!.click());
    const dialog = container.querySelector('[role="dialog"]')!;
    const mode = dialog.querySelector<HTMLInputElement>('[role="combobox"]')!;
    await selectComboboxValue(mode, 'native');
    const save = [...dialog.querySelectorAll('button')].find(el => el.textContent === '保存能力')!;
    await act(async () => save.click());
    expect(mode.value).toBe('原生');
    expect(dialog.querySelector('[role="alert"]')).not.toBeNull();
    expect(container.querySelector('.mantine-Notification-root')).toBeNull();
    failSave = false;
    await act(async () => { save.focus(); save.click(); });
    expect(dialog.querySelector('[role="alert"]')).toBeNull();
    expect(container.querySelectorAll('.mantine-Notification-root')).toHaveLength(1);
    expect(container.querySelector('.mantine-Notification-root')?.textContent).toContain('能力已保存');
    expect(container.querySelector('[role="dialog"]')).toBe(dialog);
    expect(document.activeElement).toBe(save);
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

  it('shows one model row with protocols and its line without implementation tabs or IDs', async () => {
    await renderPage('models');
    const table = container.querySelector('table')!;
    expect(table.querySelectorAll('tbody tr')).toHaveLength(1);
    expect(container.querySelector('[role="tab"]')).toBeNull();
    expect(table.textContent).toContain('model-public');
    expect(table.textContent).toContain('Source A');
    expect(table.textContent).toContain('Account A');
    expect(table.textContent).toContain('upstream-a');
    expect(table.textContent).toContain('主线路');
    const protocolPills = [...table.querySelectorAll('tbody td:nth-child(2) [data-ui="status-pill"]')];
    expect(protocolPills.map((pill) => pill.textContent)).toEqual(['Chat', 'Responses', 'Messages']);
    expect(protocolPills.map((pill) => pill.getAttribute('data-tone'))).toEqual(['success', 'muted', 'muted']);
    expect(protocolPills[0].getAttribute('title')).toBe('Chat Completions · 原生');
    expect(table.textContent).not.toContain('route-a');
    expect(table.textContent).not.toContain('选择 #');
    expect(table.textContent).not.toContain('绑定 ID');
    expect(table.textContent).not.toContain('固定主选');
  });

  it('creates through POST, retains a conflicting draft, and never treats a model name as an update ID', async () => {
    const writes = vi.fn(async (path: string, init: RequestInit) => {
      expect(path).toBe('/admin/model-routings');
      expect(init.method).toBe('POST');
      return JSON.parse(String(init.body)).public_name === logicalModel.public_name
        ? jsonResponse({ error: { code: 'conflict', message: 'Model name already exists' } }, 409)
        : jsonResponse({ data: {} }, 201);
    });
    vi.stubGlobal('fetch', vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
      if (init?.method === 'POST' || init?.method === 'PUT') return writes(String(input), init);
      return baseHandler(input);
    }));
    await renderPage('models');
    await act(async () => [...container.querySelectorAll<HTMLButtonElement>('button')].find(button => button.textContent === '新增模型')!.click());
    const name = container.querySelector<HTMLInputElement>('#model-routing-editor-form input')!;
    const setName = (value: string) => act(() => {
      Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')!.set!.call(name, value);
      name.dispatchEvent(new Event('input', { bubbles: true }));
    });
    setName(logicalModel.public_name);
    await selectComboboxValue(container.querySelector<HTMLInputElement>('input[aria-label="主线路 · 来源"]')!, 'source-a');
    await waitFor(() => !container.querySelector<HTMLInputElement>('input[aria-label="主线路 · 上游模型"]')!.disabled);
    await selectComboboxValue(container.querySelector<HTMLInputElement>('input[aria-label="主线路 · 上游模型"]')!, 'upstream-a');
    const submit = container.querySelector<HTMLButtonElement>('[role="dialog"] button[type="submit"]')!;
    await act(async () => submit.click());
    expect(writes).toHaveBeenCalledOnce();
    expect(container.querySelector('[role="dialog"] [role="alert"]')?.textContent).toContain('409');
    expect(name.value).toBe(logicalModel.public_name);
    setName(logicalModel.id);
    await act(async () => submit.click());
    expect(writes).toHaveBeenCalledTimes(2);
    expect(container.querySelector('[role="dialog"]')).toBeNull();
  });

  it.each([0, 1, null])('shows the retry limit %s without promising unlimited fallback', async (maxRetries) => {
    const accounts = ['a', 'b', 'c'].map((id) => ({ ...account, id: `account-${id}` }));
    const bindings = accounts.map((item, index) => ({ ...binding, id: index + 1, account_id: item.id }));
    vi.stubGlobal('fetch', vi.fn(async (input: RequestInfo | URL) => {
      const url = String(input);
      if (url === '/admin/logical-models') return jsonResponse({ data: [{ ...logicalModel, max_retries: maxRetries }] });
      if (url === '/admin/accounts') return jsonResponse({ data: accounts });
      if (url === '/admin/model-bindings') return jsonResponse({ data: bindings });
      if (url === '/admin/routes') return jsonResponse({ data: [{ ...route, strategy: 'ordered_fallback' }] });
      if (url === '/admin/capabilities') return jsonResponse({ ...capabilityResponse, data: bindings.map((item, index) => ({
        ...capabilityResponse.data[0], account: { account_id: item.account_id, enabled: true },
        protocols: [{ ...capabilityResponse.data[0].protocols[0], binding_id: item.id, selection: index === 0 ? 'primary' : 'fallback', selection_rank: index }],
      })) });
      return baseHandler(input);
    }));
    await renderPage('models');
    const table = container.querySelector('table')!;
    expect(table.querySelectorAll('[role="img"][aria-label="失败后"]')).toHaveLength(maxRetries === null ? 2 : 0);
    if (maxRetries !== null) {
      expect(table.textContent).toContain(maxRetries === 0 ? '仅尝试首条可用线路，请求失败后不再回退' : '最多尝试 2 条可用线路');
      expect(table.textContent).toContain('不占尝试次数');
    }
    expect(table.textContent).toContain('备用线路 2');
  });

  it('submits the complete model configuration once and prevents close or duplicate submission while saving', async () => {
    let finish!: () => void;
    const save = vi.fn(() => new Promise<Response>(resolve => { finish = () => resolve(jsonResponse({ data: {} })); }));
    vi.stubGlobal('fetch', vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
      if (String(input) === '/admin/logical-models/logical-a/routing' && init?.method === 'PUT') {
        expect(JSON.parse(String(init.body))).toEqual({
          public_name: 'model-public', display_name: 'Model A', enabled: true,
          lines: [{ source_id: 'source-a', account_id: 'account-a', upstream_model_id: 'upstream-a' }],
          request_timeout_ms: null, max_retries: null,
        });
        return save();
      }
      return baseHandler(input);
    }));
    await renderPage('models');
    await act(async () => container.querySelector<HTMLButtonElement>('button[aria-label="编辑 model-public"]')!.click());
    const submit = container.querySelector<HTMLButtonElement>('[role="dialog"] button[type="submit"]')!;
    await act(async () => submit.click());
    expect(save).toHaveBeenCalledOnce();
    expect(submit.disabled).toBe(true);
    const cancel = [...container.querySelectorAll<HTMLButtonElement>('[role="dialog"] button')].find(button => button.textContent === '取消')!;
    expect(cancel.disabled).toBe(true);
    await act(async () => { submit.click(); cancel.click(); });
    expect(save).toHaveBeenCalledOnce();
    expect(container.querySelector('[role="dialog"]')).not.toBeNull();
    await act(async () => finish());
    expect(container.querySelector('[role="dialog"]')).toBeNull();
  });

  it('retains the drawer draft on a failed save and permits retry', async () => {
    let failSave = true;
    const save = vi.fn(async () => failSave
      ? jsonResponse({ error: { code: 'save_conflict', message: 'Try saving again' } }, 409)
      : jsonResponse({ data: {} }));
    vi.stubGlobal('fetch', vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
      if (String(input).endsWith('/logical-a/routing') && init?.method === 'PUT') return save();
      return baseHandler(input);
    }));
    await renderPage('models');
    await act(async () => container.querySelector<HTMLButtonElement>('button[aria-label="编辑 model-public"]')!.click());
    const submit = container.querySelector<HTMLButtonElement>('[role="dialog"] button[type="submit"]')!;
    await act(async () => submit.click());
    expect(save).toHaveBeenCalledOnce();
    expect(container.querySelector('[role="dialog"] [role="alert"]')).not.toBeNull();
    expect(container.querySelector<HTMLInputElement>('#model-routing-editor-form input')?.value).toBe('model-public');
    expect(container.querySelector('.mantine-Notification-root')).toBeNull();
    failSave = false;
    await act(async () => submit.click());
    expect(save).toHaveBeenCalledTimes(2);
    expect(container.querySelector('[role="dialog"]')).toBeNull();
    expect(container.querySelectorAll('.mantine-Notification-root')).toHaveLength(1);
  });

  it('filters capabilities and exposes the selected protocol chain as labeled details', async () => {
    await renderPage('capabilities');
    const region = container.querySelector('section[aria-label="能力矩阵筛选"]')!;
    const statusLabel = [...region.querySelectorAll('label')].find((label) => label.textContent === '路由状态')!;
    const protocol = document.getElementById(statusLabel.htmlFor) as HTMLInputElement;
    await selectComboboxValue(protocol, 'degraded');
    const view = [...container.querySelectorAll<HTMLButtonElement>('tbody button')].find(button => button.getAttribute('aria-label')?.includes('查看'))!;
    await act(async () => view.click());
    const terms = [...container.querySelectorAll('[role="dialog"] dt')];
    expect(terms.length).toBeGreaterThan(0);
    expect(terms.every(term => term.nextElementSibling?.tagName === 'DD')).toBe(true);
    expect(container.querySelector('[role="dialog"]')?.textContent).toContain('adapter-a');
    expect(container.querySelector('[role="dialog"]')?.textContent).toContain('Messages');
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
      newKey?.focus();
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
    await act(async () => create?.click());
    await waitFor(() => document.body.textContent?.includes(oneTimeValue) ?? false);

    expect(writeText).toHaveBeenCalledWith(oneTimeValue);
    const resultDialog = container.querySelector<HTMLElement>('[role="dialog"]')!;
    await waitFor(() => resultDialog.contains(document.activeElement));
    expect(resultDialog.contains(document.activeElement)).toBe(true);
    expect(document.body.textContent).toContain(oneTimeValue);
    expect(document.body.textContent).toContain('API Key 已复制到剪贴板');
    expect(container.querySelector('.mantine-Notification-root')).toBeNull();
    expect(container.querySelectorAll('[role="status"]')).toHaveLength(1);
    expect(container.querySelector('[role="status"]')?.textContent).not.toContain(oneTimeValue);
    await act(async () => [...container.querySelectorAll('button')].find(el => el.textContent === '完成')!.click());
    await waitFor(() => document.activeElement === newKey);
    expect(document.body.textContent).not.toContain(oneTimeValue);
    expect(document.activeElement).toBe(newKey);
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
    const fetchRequest = vi.fn<typeof fetch>(async () => jsonResponse({
      error: { code: 'unauthorized', message: 'admin key required' },
    }, 401));
    vi.stubGlobal('fetch', fetchRequest);
    await renderPage('capabilities');

    expect(container.textContent).toContain('Admin Key 未通过验证');
    expect(container.textContent).toContain('unauthorized');
    expect(container.textContent).toContain('重试');
    fetchRequest.mockImplementation(baseHandler);
    await act(async () => {
      [...container.querySelectorAll('button')].find(button => button.textContent?.includes('重试'))!.click();
    });
    expect(container.querySelector('[role="alert"]')).toBeNull();
    expect(container.querySelector('tbody')?.textContent).toContain('Model A');
  });
});
