// @vitest-environment happy-dom
import type {
  Account,
  GatewayAdminResources,
  GatewayProtocol,
  ModelRoutingConfiguration,
  ModelRoutingWriteInput,
  Source,
  SourceModel,
  SourceModelCapability,
} from '@/admin-api';
import type { CatalogData } from './catalog';
import { ModelRoutingEditor } from './ModelRoutingEditor';
import { act } from 'react';
import { createRoot } from '@/test/render';
import { selectComboboxValue } from '@/test/interactions';
import { setTestLanguage } from '@/test/setup';
import { afterEach, beforeEach, describe, expect, it, vi, type Mock } from 'vitest';

const date = '2026-09-12T00:00:00Z';
const chat: GatewayProtocol = 'openai_chat_completions';
const responses: GatewayProtocol = 'openai_responses';
const messages: GatewayProtocol = 'anthropic_messages';

const source = (id: string): Source => ({
  id, display_name: id, provider_preset_id: 'synthetic', provider_preset_version: 1,
  provider_preset_snapshot: {}, base_url: 'https://provider.example',
  endpoints: { [chat]: '/chat', [responses]: '/responses', [messages]: '/messages' },
  auth_config: {}, protocol_capabilities: { [chat]: { mode: 'native' }, [responses]: { mode: 'native' } },
  enabled: true, created_at: date, updated_at: date,
});
const account = (id: string, sourceId: string): Account => ({
  id, source_id: sourceId, display_name: id, credential_configured: true, enabled: true,
  weight: 100, health_status: 'healthy', created_at: date, updated_at: date,
});
const sourceModel = (sourceId: string, upstreamModelId: string, overrides: Partial<SourceModel> = {}): SourceModel => ({
  source_id: sourceId, upstream_model_id: upstreamModelId,
  confirmation_status: 'confirmed', availability_status: 'available', raw_snapshot: {}, metadata: {}, field_sources: {},
  first_discovered_at: date, last_discovered_at: date, created_at: date, updated_at: date, ...overrides,
});
const capability = (sourceId: string, upstreamModelId: string, protocol: GatewayProtocol, overrides: Partial<SourceModelCapability> = {}): SourceModelCapability => ({
  source_id: sourceId, upstream_model_id: upstreamModelId, protocol, status: 'confirmed', mode: 'native',
  feature_capabilities: {}, field_source: 'user', observed_at: date, updated_at: date, ...overrides,
});
const lines = [
  { source_id: 'source-a', account_id: 'account-a', upstream_model_id: 'upstream-a', protocols: [chat] },
  { source_id: 'source-b', account_id: 'account-b', upstream_model_id: 'upstream-b', protocols: [chat] },
];
const configuration = (overrides: Partial<ModelRoutingConfiguration> = {}): ModelRoutingConfiguration => ({
  logical_model: {
    id: 'logical-a', public_name: 'public-a', display_name: 'Original display name', status: 'confirmed',
    enabled: true, metadata: {}, field_sources: {}, request_timeout_ms: null, max_retries: null, created_at: date, updated_at: date,
  },
  lines, protocols: [chat], strategy: 'primary_then_weighted_fallback', request_timeout_ms: null, max_retries: null,
  ...overrides,
});
const catalog = (): CatalogData => ({
  logicalModels: [configuration().logical_model],
  sources: [source('source-a'), source('source-b')],
  accounts: [account('account-a', 'source-a'), account('account-b', 'source-b')],
  bindings: [{
    id: 42, logical_model_id: 'logical-a', source_id: 'source-a', account_id: 'account-a', upstream_model_id: 'upstream-a',
    protocol: chat, status: 'confirmed', enabled: true, priority: 100, created_at: date, updated_at: date,
  }],
  routes: [{
    id: 'route-a', logical_model_id: 'logical-a', public_name: 'public-a', protocols: [chat],
    strategy: 'primary_then_weighted_fallback', allow_lossy_conversion: false, enabled: true, created_at: date, updated_at: date,
  }],
  capabilities: { version: 'v1', fact_source: 'runtime_snapshot', snapshot_revision: 7, snapshot_generated_at: date, data: [] },
});
const makeApi = () => ({
  sourceModels: vi.fn(async (sourceId: string, _filters: unknown, _signal?: AbortSignal) => [sourceModel(sourceId, sourceId === 'source-a' ? 'upstream-a' : 'upstream-b')]),
  sourceModelCapabilities: vi.fn(async (sourceId: string, modelId: string, _signal?: AbortSignal) => [capability(sourceId, modelId, chat)]),
});
function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((yes) => { resolve = yes; });
  return { promise, resolve };
}
async function waitFor(condition: () => boolean) {
  const deadline = Date.now() + 1500;
  while (!condition() && Date.now() < deadline) {
    await act(async () => { await new Promise((resolve) => setTimeout(resolve, 10)); });
  }
  expect(condition()).toBe(true);
}

describe('model routing editor', () => {
  let container: HTMLDivElement;
  let root: ReturnType<typeof createRoot>;
  let api: ReturnType<typeof makeApi>;
  let data: CatalogData;
  let onSubmit: Mock<(id: string | undefined, input: ModelRoutingWriteInput) => void>;

  beforeEach(async () => {
    globalThis.IS_REACT_ACT_ENVIRONMENT = true;
    await setTestLanguage('zh');
    container = document.createElement('div');
    document.body.appendChild(container);
    root = createRoot(container);
    api = makeApi();
    data = catalog();
    onSubmit = vi.fn<(id: string | undefined, input: ModelRoutingWriteInput) => void>();
  });
  afterEach(() => {
    act(() => root.unmount());
    container.remove();
  });

  const render = async (record?: ModelRoutingConfiguration, busy = false, error?: string) => {
    await act(async () => root.render(<ModelRoutingEditor api={api as unknown as GatewayAdminResources}
      data={data} configuration={record} busy={busy} error={error} onSubmit={onSubmit} />));
  };
  const field = (label: string) => {
    const ariaInput = [...container.querySelectorAll<HTMLInputElement>('input')].find((input) => input.getAttribute('aria-label') === label);
    if (ariaInput) return ariaInput;
    const element = [...container.querySelectorAll<HTMLLabelElement>('label')].find((item) => item.textContent === label);
    const input = element?.htmlFor ? document.getElementById(element.htmlFor) : null;
    if (!(input instanceof HTMLInputElement)) throw new Error(`Field not found: ${label}`);
    return input;
  };
  const button = (name: string) => {
    const item = [...container.querySelectorAll<HTMLButtonElement>('button')]
      .find((element) => (element.getAttribute('aria-label') ?? element.textContent) === name);
    if (!item) throw new Error(`Button not found: ${name}`);
    return item;
  };
  const setValue = (label: string, value: string) => act(() => {
    const input = field(label);
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')!.set!.call(input, value);
    input.dispatchEvent(new Event('input', { bubbles: true }));
  });
  const submit = () => act(() => container.querySelector('form')!.dispatchEvent(new Event('submit', { bubbles: true, cancelable: true })));
  const protocolText = (protocol: GatewayProtocol) => container.querySelector(`[data-protocol="${protocol}"]`)?.textContent;

  it('creates from a confirmed model and submits one ordered configuration with inherited settings', async () => {
    api.sourceModels.mockImplementation(async (sourceId) => [
      sourceModel(sourceId, 'upstream-a'),
      sourceModel(sourceId, 'pending-model', { confirmation_status: 'pending' }),
      sourceModel(sourceId, 'unavailable-model', { availability_status: 'unavailable' }),
    ]);
    await render();
    expect(api.sourceModels).not.toHaveBeenCalled();
    expect(container.querySelectorAll('li[data-line-key]')).toHaveLength(1);
    expect(container.textContent).not.toContain('运行配置');
    setValue('逻辑模型名称', '  new-model  ');
    await selectComboboxValue(field('主线路 · 来源'), 'source-a');
    await waitFor(() => !field('主线路 · 上游模型').disabled);
    expect(api.sourceModels).toHaveBeenCalledWith('source-a', { confirmationStatus: 'confirmed' }, expect.any(AbortSignal));
    expect(field('主线路 · 账号').value).toContain('account-a');
    await act(async () => field('主线路 · 上游模型').click());
    expect(document.querySelector('[role="option"][value="pending-model"]')).toBeNull();
    expect(document.querySelector('[role="option"][value="unavailable-model"]')?.getAttribute('data-combobox-disabled')).not.toBeNull();
    await selectComboboxValue(field('主线路 · 上游模型'), 'upstream-a');
    await waitFor(() => protocolText(chat)?.includes('原生') === true);
    submit();
    expect(onSubmit).toHaveBeenCalledExactlyOnceWith(undefined, {
      public_name: 'new-model', display_name: 'new-model', enabled: true,
      lines: [{ source_id: 'source-a', account_id: 'account-a', upstream_model_id: 'upstream-a' }],
      request_timeout_ms: null, max_retries: null,
    });
  });

  it('preserves model identity and display name while reordering by buttons and drag, adding and removing lines', async () => {
    const record = configuration();
    await render(record);
    await waitFor(() => !field('主线路 · 上游模型').disabled);
    expect(container.textContent).toContain('本次保存后将改为按所列顺序回退');
    expect(button('上移第 1 条线路').disabled).toBe(true);
    expect(button('下移第 2 条线路').disabled).toBe(true);
    await act(async () => button('上移第 2 条线路').click());
    expect(field('主线路 · 来源').value).toContain('source-b');
    expect(document.activeElement).toBe(button('拖动第 1 条线路以调整顺序'));
    expect(container.querySelector('[role="status"]')?.textContent).toBeTruthy();

    const transfer = { effectAllowed: '', dropEffect: '', setData: vi.fn() };
    const drag = new Event('dragstart', { bubbles: true });
    Object.defineProperty(drag, 'dataTransfer', { value: transfer });
    await act(async () => button('拖动第 1 条线路以调整顺序').dispatchEvent(drag));
    const drop = new Event('drop', { bubbles: true, cancelable: true });
    Object.defineProperty(drop, 'dataTransfer', { value: transfer });
    await act(async () => container.querySelectorAll('li[data-line-key]')[1].dispatchEvent(drop));
    expect(field('主线路 · 来源').value).toContain('source-a');
    expect(transfer.setData).toHaveBeenCalledWith('text/plain', 'line-1');

    await act(async () => button('添加备用线路').click());
    expect(container.querySelectorAll('li[data-line-key]')).toHaveLength(3);
    await act(async () => button('移除第 3 条线路').click());
    expect(container.querySelectorAll('li[data-line-key]')).toHaveLength(2);
    await act(async () => button('下移第 1 条线路').click());
    setValue('逻辑模型名称', 'renamed-model');
    setValue('超时（ms）', '60000');
    setValue('重试次数', '0');
    submit();
    expect(onSubmit).toHaveBeenCalledExactlyOnceWith('logical-a', {
      public_name: 'renamed-model', display_name: 'Original display name', enabled: true,
      lines: [
        { source_id: 'source-b', account_id: 'account-b', upstream_model_id: 'upstream-b' },
        { source_id: 'source-a', account_id: 'account-a', upstream_model_id: 'upstream-a' },
      ], request_timeout_ms: 60000, max_retries: 0,
    });
    await act(async () => button('高级信息').click());
    expect(button('高级信息').getAttribute('aria-expanded')).toBe('true');
    expect(container.textContent).toContain('Binding 42');
    expect(container.textContent).toContain('route-a');
    expect(container.textContent).toContain('模型映射');
    expect(container.querySelector('pre')?.textContent).toContain('snapshot_revision');
    expect([...container.querySelectorAll('input')].some((input) => input.value === '100')).toBe(false);
  });

  it('retains the draft after a failed save and disables edits and duplicate submissions while busy', async () => {
    const record = configuration();
    await render(record);
    await waitFor(() => !field('主线路 · 上游模型').disabled);
    setValue('逻辑模型名称', 'kept-draft');
    await act(async () => button('上移第 2 条线路').click());
    submit();
    expect(onSubmit).toHaveBeenCalledTimes(1);
    await render(record, true);
    expect(field('逻辑模型名称').disabled).toBe(true);
    expect(field('主线路 · 来源').disabled).toBe(true);
    expect(field('主线路 · 账号').disabled).toBe(true);
    expect(field('主线路 · 上游模型').disabled).toBe(true);
    expect(field('超时（ms）').disabled).toBe(true);
    expect(field('重试次数').disabled).toBe(true);
    expect(field('启用模型').disabled).toBe(true);
    expect(button('添加备用线路').disabled).toBe(true);
    expect(button('移除第 1 条线路').disabled).toBe(true);
    expect(button('拖动第 1 条线路以调整顺序').getAttribute('draggable')).toBe('false');
    submit();
    expect(onSubmit).toHaveBeenCalledTimes(1);
    await render(record, false, '模拟保存失败');
    expect(field('逻辑模型名称').value).toBe('kept-draft');
    expect(field('主线路 · 来源').value).toContain('source-b');
    expect(container.textContent).toContain('模拟保存失败');
    submit();
    expect(onSubmit).toHaveBeenCalledTimes(2);
  });

  it('resets dependent selections and aborts stale SourceModel and capability queries when the source changes', async () => {
    const oldModels = deferred<SourceModel[]>();
    const oldCapabilities = deferred<SourceModelCapability[]>();
    api.sourceModels.mockImplementation(async (sourceId) => sourceId === 'source-a'
      ? oldModels.promise : [sourceModel('source-b', 'upstream-b')]);
    api.sourceModelCapabilities.mockImplementation(async (sourceId, upstreamModelId) => sourceId === 'source-a'
      ? oldCapabilities.promise : [capability(sourceId, upstreamModelId, responses)]);
    await render(configuration({ lines: [lines[0]] }));
    await waitFor(() => api.sourceModels.mock.calls.length === 1 && api.sourceModelCapabilities.mock.calls.length === 1);
    const oldModelSignal = api.sourceModels.mock.calls[0][2]!;
    const oldCapabilitySignal = api.sourceModelCapabilities.mock.calls[0][2]!;
    await selectComboboxValue(field('主线路 · 来源'), 'source-b');
    expect(oldModelSignal.aborted).toBe(true);
    expect(oldCapabilitySignal.aborted).toBe(true);
    expect(field('主线路 · 账号').value).toContain('account-b');
    expect(field('主线路 · 上游模型').value).toBe('');
    await waitFor(() => !field('主线路 · 上游模型').disabled);
    await selectComboboxValue(field('主线路 · 上游模型'), 'upstream-b');
    await waitFor(() => protocolText(responses)?.includes('原生') === true);
    await act(async () => {
      oldModels.resolve([sourceModel('source-a', 'late-old-model')]);
      oldCapabilities.resolve([capability('source-a', 'upstream-a', chat)]);
    });
    expect(field('主线路 · 上游模型').value).toBe('upstream-b');
    expect(protocolText(chat)).toContain('未知');
    expect(protocolText(responses)).toContain('原生');
    expect(container.textContent).not.toContain('late-old-model');
  });

  it('does not infer native or converted support from Source defaults, pending declarations, or an unregistered adapter', async () => {
    api.sourceModelCapabilities.mockResolvedValue([
      capability('source-a', 'upstream-a', chat, { status: 'pending' }),
      capability('source-a', 'upstream-a', responses, { mode: 'adapter', source_protocol: chat, adapter: 'not-registered' }),
      capability('source-a', 'upstream-a', messages, { mode: 'unsupported' }),
    ]);
    await render(configuration({ lines: [lines[0]] }));
    await waitFor(() => protocolText(messages)?.includes('不支持') === true);
    expect(protocolText(chat)).toContain('未知');
    expect(protocolText(responses)).toContain('不可用');
    expect(protocolText(responses)).not.toContain('协议转换');
    expect(container.querySelector(`[data-protocol="${chat}"] [data-tone="success"]`)).toBeNull();
    expect(container.querySelectorAll('[data-protocol] input')).toHaveLength(0);
  });

  it('does not label a cooling account healthy or a native declaration without an endpoint usable', async () => {
    data.accounts[0].cooldown_until = '2999-01-01T00:00:00Z';
    data.sources[0].endpoints = {};
    await render(configuration({ lines: [lines[0]] }));
    await waitFor(() => protocolText(chat)?.includes('不可用') === true);
    const line = container.querySelector('li[data-line-key]')!;
    expect(line.textContent).toContain('不可用');
    expect(line.textContent).not.toContain('正常');
    expect(line.querySelector('[data-tone="success"]')).toBeNull();
    expect(container.querySelector(`[data-protocol="${chat}"] [data-tone="success"]`)).toBeNull();
  });

  it('shows mixed native and converted support only when the converted line has a matching published path', async () => {
    api.sourceModelCapabilities.mockImplementation(async (sourceId, upstreamModelId) => [
      capability(sourceId, upstreamModelId, responses, sourceId === 'source-b'
        ? { mode: 'adapter', source_protocol: chat, adapter: 'synthetic-adapter' } : {}),
    ]);
    data.capabilities.data = [{
      route_id: 'runtime-b', source: { source_id: 'source-b' }, account: { account_id: 'account-b', enabled: true },
      model: 'public-a', model_display_name: 'Original display name', upstream_model_id: 'upstream-b',
      protocols: [{
        protocol_in: responses, protocol_upstream: chat, status: 'routable', mode: 'adapter', adapter: 'synthetic-adapter',
        conversion_chain: [{ protocol_from: responses, protocol_to: chat, mode: 'adapter', adapter: 'synthetic-adapter' }],
        effective_capabilities: {
          streaming: 'translated', tools: 'unsupported', tool_streaming: 'unsupported', thinking: 'unsupported',
          web_search: 'unsupported', file_search: 'unsupported', vision: 'unsupported', usage: 'translated',
        }, degraded: false, degraded_features: [],
      }],
    }];
    await render(configuration());
    await waitFor(() => protocolText(responses)?.includes('原生 / 转换') === true);
    expect(container.querySelector(`[data-protocol="${responses}"] [data-tone="warning"]`)).not.toBeNull();
  });

  it('validates required fields, whole-number settings, duplicate lines, and an empty routing draft', async () => {
    await render(configuration({ lines: [lines[0], lines[0]] }));
    await waitFor(() => !field('主线路 · 上游模型').disabled);
    setValue('逻辑模型名称', '');
    setValue('超时（ms）', '0');
    setValue('重试次数', '-1');
    submit();
    expect(onSubmit).not.toHaveBeenCalled();
    expect(field('逻辑模型名称').getAttribute('aria-invalid')).toBe('true');
    expect(field('超时（ms）').getAttribute('aria-invalid')).toBe('true');
    expect(field('重试次数').getAttribute('aria-invalid')).toBe('true');
    setValue('逻辑模型名称', 'valid-name');
    setValue('超时（ms）', '');
    setValue('重试次数', '');
    submit();
    expect(onSubmit).not.toHaveBeenCalled();
    expect(container.textContent).toContain('同一来源、账号和上游模型不能重复添加');
    await act(async () => button('移除第 2 条线路').click());
    await act(async () => button('移除第 1 条线路').click());
    submit();
    expect(onSubmit).not.toHaveBeenCalled();
    expect(container.textContent).toContain('启用模型至少配置一条上游线路');
    expect(button('添加线路')).toBeTruthy();
  });

  it('saves a disabled unavailable model with no lines as one atomic configuration', async () => {
    const record = configuration({
      logical_model: { ...configuration().logical_model, enabled: false, status: 'unavailable' },
      lines: [], protocols: [],
    });
    data.sources = [];
    data.accounts = [];
    await render(record);
    setValue('逻辑模型名称', 'disabled-model');
    submit();
    expect(onSubmit).toHaveBeenCalledExactlyOnceWith('logical-a', {
      public_name: 'disabled-model', display_name: 'Original display name', enabled: false,
      lines: [], request_timeout_ms: null, max_retries: null,
    });
    expect(api.sourceModels).not.toHaveBeenCalled();
    expect(api.sourceModelCapabilities).not.toHaveBeenCalled();
  });

  it('keeps the selected model on a query failure and offers an in-place retry', async () => {
    api.sourceModels.mockRejectedValueOnce(new Error('synthetic lookup failure'));
    await render(configuration({ lines: [lines[0]] }));
    await waitFor(() => container.querySelector('[role="alert"]') !== null);
    expect(field('主线路 · 上游模型').value).toContain('upstream-a');
    expect(field('逻辑模型名称').value).toBe('public-a');
    expect(protocolText(chat)).toContain('未知');
    await act(async () => button('重试').click());
    await waitFor(() => !field('主线路 · 上游模型').disabled);
    expect(api.sourceModels).toHaveBeenCalledTimes(2);
    expect(field('主线路 · 上游模型').value).toBe('upstream-a');
    expect(protocolText(chat)).toContain('原生');
  });
});
