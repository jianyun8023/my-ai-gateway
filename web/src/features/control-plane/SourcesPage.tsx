import { useCallback, useEffect, useMemo, useState, type FormEvent } from 'react';
import type {
  Account,
  AccountWriteInput,
  AdminErrorShape,
  ConnectionTestResult,
  GatewayAdminResources,
  GatewayProtocol,
  ProviderPreset,
  ProviderPresetDiff,
  Source,
  SourceCreateInput,
  SourceProtocolMode,
  SourceWriteInput,
} from '@/admin-api';
import { GATEWAY_PROTOCOLS, normalizeAdminError } from '@/admin-api';
import { Button } from '@/components/ui/Button';
import { Card } from '@/components/ui/Card';
import { Modal } from '@/components/ui/Modal';
import {
  IconCircleCheck,
  IconEye,
  IconPencil,
  IconPlay,
  IconPlus,
  IconPower,
  IconRefreshCw,
  IconTrash2,
  IconTriangleAlert,
} from '@/components/ui/icons';
import {
  CheckboxField,
  ConfirmDialog,
  DetailItem,
  DetailList,
  DrawerSection,
  EmptyTable,
  ErrorState,
  FormError,
  FormGrid,
  IconButton,
  LoadingState,
  PROTOCOL_LABELS,
  PageActions,
  ProtocolPill,
  SegmentedTabs,
  SelectField,
  StatusPill,
  SuccessNotice,
  TableScroll,
  TextAreaField,
  TextField,
  Toggle,
  formatDateTime,
  formatJsonValue,
} from './shared';
import { useAdminQuery } from './useAdminQuery';
import styles from './ControlPlane.module.scss';

interface SourcesPageProps {
  api: GatewayAdminResources;
  refreshRevision?: number;
  onBusyChange?: (busy: boolean) => void;
}

interface SourcesData {
  sources: Source[];
  accounts: Account[];
  presets: ProviderPreset[];
}

type SourceEditor = { kind: 'source'; record?: Source };
type AccountEditor = { kind: 'account'; record?: Account };
type Editor = SourceEditor | AccountEditor;
type DeleteTarget = { kind: 'source'; record: Source } | { kind: 'account'; record: Account };
type SourcesTab = 'sources' | 'accounts';

const protocolModeTone = (mode: SourceProtocolMode | undefined) => {
  if (mode === 'native') return 'success' as const;
  if (mode === 'adapter') return 'warning' as const;
  if (mode === 'unsupported') return 'muted' as const;
  return 'accent' as const;
};

const protocolModeLabel = (mode: SourceProtocolMode | undefined) => {
  if (mode === 'native') return 'native';
  if (mode === 'adapter') return 'adapter';
  if (mode === 'unsupported') return 'unsupported';
  return 'unknown';
};

const credentialLabel = (account: Account) => (
  account.credential_configured ? 'Configured' : 'Not configured'
);

const presetEndpoints = (preset?: ProviderPreset): Partial<Record<GatewayProtocol, string>> => {
  const endpoints: Partial<Record<GatewayProtocol, string>> = {};
  for (const protocol of GATEWAY_PROTOCOLS) {
    const endpoint = preset?.definition.protocols?.[protocol]?.endpoint;
    if (endpoint) endpoints[protocol] = endpoint;
  }
  return endpoints;
};

const presetCapabilities = (preset?: ProviderPreset): Source['protocol_capabilities'] => {
  const capabilities: Source['protocol_capabilities'] = {};
  for (const protocol of GATEWAY_PROTOCOLS) {
    const definition = preset?.definition.protocols?.[protocol];
    if (definition) {
      capabilities[protocol] = {
        mode: definition.mode,
        source_protocol: definition.source_protocol,
        adapter: definition.adapter,
        features: definition.default_capabilities,
      };
    }
  }
  return capabilities;
};

const presetAuthConfig = (preset?: ProviderPreset): Source['auth_config'] => {
  const header = preset?.definition.credential_header;
  return header
    ? { credential_header: header, default_headers: preset?.definition.default_headers ?? {} }
    : {};
};

const authField = (authConfig: Source['auth_config'], field: 'header' | 'prefix'): string => {
  const credentialHeader = authConfig.credential_header;
  if (!credentialHeader || typeof credentialHeader !== 'object') return '';
  const value = (credentialHeader as Record<string, unknown>)[field];
  return typeof value === 'string' ? value : '';
};

const defaultHeadersJson = (authConfig: Source['auth_config']): string => {
  const value = authConfig.default_headers;
  return JSON.stringify(value && typeof value === 'object' ? value : {}, null, 2);
};

function SourceForm({
  record,
  presets,
  busy,
  error,
  onSubmit,
}: {
  record?: Source;
  presets: ProviderPreset[];
  busy: boolean;
  error?: string;
  onSubmit: (input: SourceCreateInput | SourceWriteInput) => void;
}) {
  const orderedPresets = useMemo(
    () => [...presets].sort((left, right) => left.id.localeCompare(right.id) || right.version - left.version),
    [presets],
  );
  const initialPreset = record
    ? orderedPresets.find((preset) => preset.id === record.provider_preset_id && preset.version === record.provider_preset_version)
    : orderedPresets.find((preset) => preset.id !== 'custom') ?? orderedPresets[0];
  const [id, setId] = useState(record?.id ?? '');
  const [displayName, setDisplayName] = useState(record?.display_name ?? '');
  const [presetKey, setPresetKey] = useState(
    initialPreset ? `${initialPreset.id}@${initialPreset.version}` : '',
  );
  const [baseUrl, setBaseUrl] = useState(record?.base_url ?? initialPreset?.definition.default_base_url ?? '');
  const [endpoints, setEndpoints] = useState<Partial<Record<GatewayProtocol, string>>>(() => (
    record?.endpoints
    ?? presetEndpoints(initialPreset)
  ));
  const [capabilities, setCapabilities] = useState<Source['protocol_capabilities']>(() => (
    record?.protocol_capabilities
    ?? presetCapabilities(initialPreset)
  ));
  const initialAuthConfig = record?.auth_config ?? presetAuthConfig(initialPreset);
  const [authHeader, setAuthHeader] = useState(() => authField(initialAuthConfig, 'header'));
  const [authPrefix, setAuthPrefix] = useState(() => authField(initialAuthConfig, 'prefix'));
  const [defaultHeaders, setDefaultHeaders] = useState(() => defaultHeadersJson(initialAuthConfig));
  const [enabled, setEnabled] = useState(record?.enabled ?? true);
  const [validationError, setValidationError] = useState('');

  const selectedPreset = orderedPresets.find((preset) => `${preset.id}@${preset.version}` === presetKey);

  const selectPreset = (nextKey: string) => {
    setPresetKey(nextKey);
    const preset = orderedPresets.find((candidate) => `${candidate.id}@${candidate.version}` === nextKey);
    if (!preset || record) return;
    setBaseUrl(preset.definition.default_base_url ?? '');
    setEndpoints(presetEndpoints(preset));
    setCapabilities(presetCapabilities(preset));
    const authConfig = presetAuthConfig(preset);
    setAuthHeader(authField(authConfig, 'header'));
    setAuthPrefix(authField(authConfig, 'prefix'));
    setDefaultHeaders(defaultHeadersJson(authConfig));
  };

  const changeCapabilityMode = (protocol: GatewayProtocol, mode: SourceProtocolMode) => {
    setCapabilities((current) => {
      const previous = current[protocol];
      if (mode !== 'adapter') {
        return { ...current, [protocol]: { mode, features: previous?.features } };
      }
      return {
        ...current,
        [protocol]: {
          mode,
          source_protocol: previous?.source_protocol ?? GATEWAY_PROTOCOLS.find((candidate) => candidate !== protocol),
          adapter: previous?.adapter ?? '',
          features: previous?.features,
        },
      };
    });
  };

  const submit = (event: FormEvent) => {
    event.preventDefault();
    if (!id.trim() || !displayName.trim() || !presetKey || !baseUrl.trim()) {
      setValidationError('ID、显示名称、ProviderPreset 和 Base URL 均为必填项。');
      return;
    }
    if (!selectedPreset && !record) {
      setValidationError('请选择有效的 ProviderPreset 版本。');
      return;
    }
    const invalidAdapter = GATEWAY_PROTOCOLS.some((protocol) => {
      const capability = capabilities[protocol];
      return capability?.mode === 'adapter'
        && (!capability.source_protocol || !capability.adapter?.trim());
    });
    if (invalidAdapter) {
      setValidationError('Adapter 模式必须同时指定上游协议和 Adapter 名称。');
      return;
    }
    let parsedHeaders: unknown;
    try {
      parsedHeaders = JSON.parse(defaultHeaders || '{}');
    } catch {
      setValidationError('默认 Header 必须是有效的 JSON 对象。');
      return;
    }
    if (!parsedHeaders || typeof parsedHeaders !== 'object' || Array.isArray(parsedHeaders)) {
      setValidationError('默认 Header 必须是 JSON 对象。');
      return;
    }
    if (!authHeader.trim()) {
      setValidationError('凭据 Header 不能为空。');
      return;
    }
    if (!Object.values(parsedHeaders).every((value) => typeof value === 'string')) {
      setValidationError('默认 Header 的值必须是字符串。');
      return;
    }
    const authConfig = {
      credential_header: { header: authHeader.trim(), prefix: authPrefix },
      default_headers: parsedHeaders as Record<string, string>,
    };
    setValidationError('');
    if (record) {
      onSubmit({
        id: record.id,
        display_name: displayName.trim(),
        provider_preset_id: record.provider_preset_id,
        provider_preset_version: record.provider_preset_version,
        base_url: baseUrl.trim(),
        endpoints,
        auth_config: authConfig,
        protocol_capabilities: capabilities,
        enabled,
      });
      return;
    }
    onSubmit({
      id: id.trim(),
      display_name: displayName.trim(),
      provider_preset_id: selectedPreset!.id,
      provider_preset_version: selectedPreset!.version,
      base_url: baseUrl.trim(),
      endpoint_overrides: endpoints,
      protocol_capabilities: capabilities,
      auth_config: authConfig,
      enabled,
    });
  };

  return (
    <form id="source-editor-form" className={styles.page} onSubmit={submit}>
      <FormGrid>
        <TextField label="Source ID" value={id} disabled={Boolean(record) || busy} onChange={(event) => setId(event.target.value)} autoComplete="off" />
        <TextField label="显示名称" value={displayName} disabled={busy} onChange={(event) => setDisplayName(event.target.value)} autoComplete="off" />
        <SelectField label="ProviderPreset" value={presetKey} disabled={Boolean(record) || busy || orderedPresets.length === 0} onChange={(event) => selectPreset(event.target.value)}>
          {orderedPresets.length === 0 && <option value="">无可用预设</option>}
          {orderedPresets.map((preset) => <option key={`${preset.id}@${preset.version}`} value={`${preset.id}@${preset.version}`}>{preset.display_name} · {preset.id}@{preset.version}</option>)}
        </SelectField>
        <TextField label="Base URL" type="url" value={baseUrl} disabled={busy} onChange={(event) => setBaseUrl(event.target.value)} autoComplete="off" />
        {(['openai_chat_completions', 'openai_responses', 'anthropic_messages'] as const).map((protocol) => (
          <TextField
            key={protocol}
            label={`${PROTOCOL_LABELS[protocol]} endpoint`}
            value={endpoints[protocol] ?? ''}
            disabled={busy}
            onChange={(event) => setEndpoints((current) => ({ ...current, [protocol]: event.target.value }))}
            autoComplete="off"
          />
        ))}
        <div className={styles.fullWidth}>
          <CheckboxField checked={enabled} disabled={busy} onChange={setEnabled} label="启用 Source" />
        </div>
      </FormGrid>
      <DrawerSection title="三协议能力声明">
        <div className={styles.protocolCapabilityEditor}>
          {GATEWAY_PROTOCOLS.map((protocol) => {
            const capability = capabilities[protocol];
            const mode = capability?.mode ?? 'unknown';
            return (
              <div key={protocol}>
                <strong>{PROTOCOL_LABELS[protocol]}</strong>
                <SelectField label="Mode" value={mode} disabled={busy} onChange={(event) => changeCapabilityMode(protocol, event.target.value as SourceProtocolMode)}>
                  <option value="native">native</option>
                  <option value="adapter">adapter</option>
                  <option value="unsupported">unsupported</option>
                  <option value="unknown" disabled>unknown</option>
                </SelectField>
                {mode === 'adapter' && (
                  <>
                    <SelectField label="Upstream protocol" value={capability?.source_protocol ?? ''} disabled={busy} onChange={(event) => setCapabilities((current) => ({
                      ...current,
                      [protocol]: {
                        ...current[protocol],
                        mode: 'adapter',
                        source_protocol: event.target.value as GatewayProtocol,
                        adapter: current[protocol]?.adapter ?? '',
                      },
                    }))}>
                      {GATEWAY_PROTOCOLS.filter((candidate) => candidate !== protocol).map((candidate) => <option key={candidate} value={candidate}>{PROTOCOL_LABELS[candidate]}</option>)}
                    </SelectField>
                    <TextField label="Adapter" value={capability?.adapter ?? ''} disabled={busy} onChange={(event) => setCapabilities((current) => ({
                      ...current,
                      [protocol]: {
                        ...current[protocol],
                        mode: 'adapter',
                        source_protocol: current[protocol]?.source_protocol ?? GATEWAY_PROTOCOLS.find((candidate) => candidate !== protocol),
                        adapter: event.target.value,
                      },
                    }))} autoComplete="off" spellCheck={false} />
                  </>
                )}
              </div>
            );
          })}
        </div>
      </DrawerSection>
      <DrawerSection title="认证模板">
        <FormGrid>
          <TextField label="凭据 Header" value={authHeader} disabled={busy} onChange={(event) => setAuthHeader(event.target.value)} autoComplete="off" spellCheck={false} />
          <TextField label="Header Prefix" value={authPrefix} disabled={busy} onChange={(event) => setAuthPrefix(event.target.value)} autoComplete="off" spellCheck={false} />
          <TextAreaField label="默认 Headers JSON" value={defaultHeaders} disabled={busy} onChange={(event) => setDefaultHeaders(event.target.value)} spellCheck={false} />
        </FormGrid>
      </DrawerSection>
      <FormError message={validationError || error} />
    </form>
  );
}

function AccountForm({
  record,
  sources,
  busy,
  error,
  onSubmit,
}: {
  record?: Account;
  sources: Source[];
  busy: boolean;
  error?: string;
  onSubmit: (input: AccountWriteInput) => void;
}) {
  const [id, setId] = useState(record?.id ?? '');
  const [sourceId, setSourceId] = useState(record?.source_id ?? sources[0]?.id ?? '');
  const [displayName, setDisplayName] = useState(record?.display_name ?? '');
  // Existing credential references are intentionally not echoed into the DOM.
  // Editing an account requires explicitly providing the environment variable again.
  const [credentialEnv, setCredentialEnv] = useState('');
  const [weight, setWeight] = useState(record?.weight ?? 100);
  const [enabled, setEnabled] = useState(record?.enabled ?? true);
  const [validationError, setValidationError] = useState('');

  const submit = (event: FormEvent) => {
    event.preventDefault();
    if (!id.trim() || !sourceId || !displayName.trim() || !credentialEnv.trim()) {
      setValidationError('ID、Source、显示名称和凭据环境变量均为必填项。');
      return;
    }
    if (!Number.isInteger(weight) || weight <= 0) {
      setValidationError('权重必须是正整数。');
      return;
    }
    setValidationError('');
    onSubmit({
      id: id.trim(),
      source_id: sourceId,
      display_name: displayName.trim(),
      credential_env: credentialEnv.trim(),
      credential_ciphertext: null,
      enabled,
      weight,
    });
  };

  return (
    <form id="account-editor-form" className={styles.page} onSubmit={submit}>
      <FormGrid>
        <TextField label="Account ID" value={id} disabled={Boolean(record) || busy} onChange={(event) => setId(event.target.value)} autoComplete="off" />
        <TextField label="显示名称" value={displayName} disabled={busy} onChange={(event) => setDisplayName(event.target.value)} autoComplete="off" />
        <SelectField label="Source" value={sourceId} disabled={busy} onChange={(event) => setSourceId(event.target.value)}>
          {sources.map((source) => <option key={source.id} value={source.id}>{source.display_name} · {source.id}</option>)}
        </SelectField>
        <TextField label="凭据环境变量" hint={record ? '现有引用不会回显；如需保存请重新输入环境变量名。' : '仅提交环境变量名，不提交凭据值。'} value={credentialEnv} disabled={busy} onChange={(event) => setCredentialEnv(event.target.value)} autoComplete="off" spellCheck={false} />
        <TextField label="Fallback 权重" type="number" min={1} step={1} value={weight} disabled={busy} onChange={(event) => setWeight(Number(event.target.value))} />
        <div className={styles.field}>
          <label>凭据状态</label>
          <StatusPill tone={record?.credential_configured ? 'success' : 'muted'}>{record ? credentialLabel(record) : '提交后验证'}</StatusPill>
        </div>
        <div className={styles.fullWidth}>
          <CheckboxField checked={enabled} disabled={busy} onChange={setEnabled} label="启用 Account" />
        </div>
      </FormGrid>
      <FormError message={validationError || error} />
    </form>
  );
}

function SourceDetailDrawer({
  source,
  accounts,
  api,
  onClose,
  onEdit,
}: {
  source: Source;
  accounts: Account[];
  api: GatewayAdminResources;
  onClose: () => void;
  onEdit: () => void;
}) {
  const [open, setOpen] = useState(true);
  const [diff, setDiff] = useState<ProviderPresetDiff>();
  const [diffError, setDiffError] = useState<AdminErrorShape>();
  const [diffLoading, setDiffLoading] = useState(true);
  const enabledAccounts = accounts.filter((account) => account.enabled);
  const [accountId, setAccountId] = useState(enabledAccounts[0]?.id ?? '');
  const [testModel, setTestModel] = useState('');
  const [testBusy, setTestBusy] = useState<GatewayProtocol>();
  const [testResults, setTestResults] = useState<Partial<Record<GatewayProtocol, ConnectionTestResult>>>({});
  const [testError, setTestError] = useState<AdminErrorShape>();

  useEffect(() => {
    if (open) return;
    const timer = window.setTimeout(onClose, 380);
    return () => window.clearTimeout(timer);
  }, [onClose, open]);

  const loadDiff = useCallback(async (signal: AbortSignal) => {
    setDiffLoading(true);
    setDiffError(undefined);
    try {
      const result = await api.sourcePresetDiff(source.id, signal);
      if (!signal.aborted) setDiff(result);
    } catch (error) {
      if (!signal.aborted) setDiffError(normalizeAdminError(error));
    } finally {
      if (!signal.aborted) setDiffLoading(false);
    }
  }, [api, source.id]);

  useEffect(() => {
    const controller = new AbortController();
    void loadDiff(controller.signal);
    return () => controller.abort();
  }, [loadDiff]);

  const runTest = async (protocol: GatewayProtocol) => {
    if (!accountId || testBusy) return;
    setTestBusy(protocol);
    setTestError(undefined);
    try {
      const result = await api.testConnection(source.id, {
        account_id: accountId,
        protocol,
        model: testModel.trim() || undefined,
        requested_by: 'admin-ui',
      });
      setTestResults((current) => ({ ...current, [protocol]: result }));
    } catch (error) {
      setTestError(normalizeAdminError(error));
    } finally {
      setTestBusy(undefined);
    }
  };

  return (
    <Modal
      open={open}
      variant="drawer"
      width={540}
      title="Source 详情"
      onClose={() => setOpen(false)}
      footer={(
        <>
          <Button variant="secondary" onClick={() => setOpen(false)}>关闭</Button>
          <Button variant="primary" onClick={onEdit}><IconPencil size={14} />编辑 Source</Button>
        </>
      )}
    >
      <DrawerSection title="基本信息">
        <DetailList>
          <DetailItem label="Source"><span className={styles.mono}>{source.id}</span></DetailItem>
          <DetailItem label="显示名称">{source.display_name}</DetailItem>
          <DetailItem label="ProviderPreset"><span className={styles.mono}>{source.provider_preset_id}@{source.provider_preset_version}</span></DetailItem>
          <DetailItem label="Base URL"><span className={styles.mono}>{source.base_url}</span></DetailItem>
          <DetailItem label="状态"><StatusPill tone={source.enabled ? 'success' : 'muted'}>{source.enabled ? 'Enabled' : 'Disabled'}</StatusPill></DetailItem>
          <DetailItem label="更新时间">{formatDateTime(source.updated_at)}</DetailItem>
        </DetailList>
      </DrawerSection>

      <DrawerSection title="协议快照">
        <DetailList>
          {(['openai_chat_completions', 'openai_responses', 'anthropic_messages'] as const).map((protocol) => {
            const capability = source.protocol_capabilities[protocol];
            const endpointProtocol = capability?.source_protocol ?? protocol;
            return (
              <DetailItem key={protocol} label={PROTOCOL_LABELS[protocol]}>
                <span className={styles.inlineActions}>
                  <StatusPill tone={protocolModeTone(capability?.mode)}>{protocolModeLabel(capability?.mode)}</StatusPill>
                  <span className={styles.mono}>{source.endpoints[endpointProtocol] ?? 'endpoint 未配置'}{capability?.source_protocol && <small className={styles.blockMeta}>upstream endpoint</small>}</span>
                  {capability?.source_protocol && <span className={styles.secondaryText}>← {PROTOCOL_LABELS[capability.source_protocol]}</span>}
                </span>
              </DetailItem>
            );
          })}
        </DetailList>
      </DrawerSection>

      <DrawerSection title="ProviderPreset 差异">
        {diffLoading ? <LoadingState label="正在比较预设…" /> : diffError ? <ErrorState error={diffError} onRetry={() => void loadDiff(new AbortController().signal)} /> : !diff || diff.changes.length === 0 ? (
          <EmptyTable title="当前快照与最新预设一致" />
        ) : (
          <div className={styles.page}>
            <div className={styles.inlineActions}>
              <StatusPill tone="accent">v{diff.source_version} → v{diff.latest_version}</StatusPill>
              <span className={styles.secondaryText}>{diff.changes.length} 项差异</span>
            </div>
            <TableScroll label="ProviderPreset 差异">
              <table className={styles.table}>
                <thead><tr><th>Path</th><th>类型</th><th>原值</th><th>新值</th></tr></thead>
                <tbody>{diff.changes.map((change) => (
                  <tr key={`${change.kind}:${change.path}`}>
                    <td><code>{change.path}</code></td>
                    <td><StatusPill tone={change.kind === 'added' ? 'success' : change.kind === 'missing' ? 'danger' : 'warning'}>{change.kind}</StatusPill></td>
                    <td><code>{formatJsonValue(change.before)}</code></td>
                    <td><code>{formatJsonValue(change.after)}</code></td>
                  </tr>
                ))}</tbody>
              </table>
            </TableScroll>
          </div>
        )}
      </DrawerSection>

      <DrawerSection title="三协议连接测试">
        {enabledAccounts.length === 0 ? <EmptyTable title="没有可用 Account" description="先创建并启用属于此 Source 的 Account。" /> : (
          <div className={styles.page}>
            <FormGrid>
              <SelectField label="Account" value={accountId} disabled={Boolean(testBusy)} onChange={(event) => setAccountId(event.target.value)}>
                {enabledAccounts.map((account) => <option key={account.id} value={account.id}>{account.display_name} · {account.id}</option>)}
              </SelectField>
              <TextField label="测试模型（可选）" value={testModel} disabled={Boolean(testBusy)} onChange={(event) => setTestModel(event.target.value)} autoComplete="off" />
            </FormGrid>
            {testError && <ErrorState error={testError} />}
            <div className={styles.protocolTestGrid}>
              {(['openai_chat_completions', 'openai_responses', 'anthropic_messages'] as const).map((protocol) => {
                const result = testResults[protocol];
                const succeeded = result?.status === 'succeeded';
                return (
                  <div key={protocol} className={styles.protocolTestRow}>
                    <ProtocolPill protocol={protocol} />
                    {result && (
                      <span className={styles.primaryText}>
                        <strong>{succeeded ? <><IconCircleCheck size={14} /> succeeded</> : <><IconTriangleAlert size={14} /> {result.status}</>}</strong>
                        <small>{result.mode} · {PROTOCOL_LABELS[result.upstream_protocol]} · {result.http_status ?? 'no HTTP'} · {result.latency_ms} ms</small>
                        {result.error_code && <small>{result.error_code}: {result.error_message}</small>}
                      </span>
                    )}
                    <Button size="sm" variant="secondary" loading={testBusy === protocol} disabled={Boolean(testBusy && testBusy !== protocol)} onClick={() => void runTest(protocol)}>
                      <IconPlay size={14} />测试
                    </Button>
                  </div>
                );
              })}
            </div>
          </div>
        )}
      </DrawerSection>
    </Modal>
  );
}

export function SourcesPage({ api, refreshRevision = 0, onBusyChange }: SourcesPageProps) {
  const [tab, setTab] = useState<SourcesTab>('sources');
  const [editor, setEditor] = useState<Editor>();
  const [deleteTarget, setDeleteTarget] = useState<DeleteTarget>();
  const [selectedSourceId, setSelectedSourceId] = useState<string>();
  const [mutationBusy, setMutationBusy] = useState(false);
  const [mutationError, setMutationError] = useState<AdminErrorShape>();
  const [notice, setNotice] = useState('');

  const load = useCallback(async (signal: AbortSignal): Promise<SourcesData> => {
    const [sources, accounts, presets] = await Promise.all([
      api.sources(signal),
      api.accounts(signal),
      api.providerPresets(signal),
    ]);
    return { sources, accounts, presets };
  }, [api]);
  const query = useAdminQuery({ load, refreshRevision, onBusyChange });
  const data = query.data;
  const selectedSource = data?.sources.find((source) => source.id === selectedSourceId);

  const mutate = async (operation: () => Promise<unknown>, successMessage: string) => {
    if (mutationBusy) return;
    setMutationBusy(true);
    setMutationError(undefined);
    onBusyChange?.(true);
    try {
      await operation();
      setEditor(undefined);
      setDeleteTarget(undefined);
      setNotice(successMessage);
      query.reload();
    } catch (error) {
      setMutationError(normalizeAdminError(error));
    } finally {
      setMutationBusy(false);
      onBusyChange?.(false);
    }
  };

  const submitSource = (input: SourceCreateInput | SourceWriteInput) => {
    const record = editor?.kind === 'source' ? editor.record : undefined;
    void mutate(
      () => record
        ? api.updateSource(record.id, input as SourceWriteInput)
        : api.createSource(input as SourceCreateInput),
      record ? `Source ${record.id} 已更新。` : `Source ${input.id} 已创建。`,
    );
  };

  const submitAccount = (input: AccountWriteInput) => {
    const record = editor?.kind === 'account' ? editor.record : undefined;
    void mutate(
      () => record ? api.updateAccount(record.id, input) : api.createAccount(input),
      record ? `Account ${record.id} 已更新。` : `Account ${input.id} 已创建。`,
    );
  };

  const confirmDelete = () => {
    if (!deleteTarget) return;
    void mutate(
      () => deleteTarget.kind === 'source'
        ? api.deleteSource(deleteTarget.record.id)
        : api.deleteAccount(deleteTarget.record.id),
      `${deleteTarget.kind === 'source' ? 'Source' : 'Account'} ${deleteTarget.record.id} 已删除。`,
    );
  };

  if (query.loading && !data) return <LoadingState label="正在加载 Sources 与 Accounts…" />;
  if (query.error && !data) return <ErrorState error={query.error} onRetry={query.reload} />;
  if (!data) return null;

  return (
    <section className={styles.page} data-od-id="page-sources">
      <PageActions>
        <SegmentedTabs
          value={tab}
          label="Source 管理资源"
          options={[
            { value: 'sources', label: 'Sources', count: data.sources.length },
            { value: 'accounts', label: 'Accounts', count: data.accounts.length },
          ]}
          onChange={setTab}
        />
        <div className={styles.rowActions}>
          <Button variant="secondary" onClick={query.reload} loading={query.refreshing}><IconRefreshCw size={14} />刷新</Button>
          <Button variant="primary" onClick={() => setEditor(tab === 'sources' ? { kind: 'source' } : { kind: 'account' })} disabled={tab === 'accounts' && data.sources.length === 0}>
            <IconPlus size={14} />{tab === 'sources' ? '新增 Source' : '新增 Account'}
          </Button>
        </div>
      </PageActions>

      <SuccessNotice message={notice} onDismiss={() => setNotice('')} />
      {query.error && <ErrorState error={query.error} onRetry={query.reload} />}
      {mutationError && !editor && !deleteTarget && <ErrorState error={mutationError} />}

      {tab === 'sources' ? (
        data.sources.length === 0 ? <EmptyTable title="尚未配置 Source" description="从 ProviderPreset 创建第一个 Source。" /> : (
          <Card variant="flush" title="Sources" subtitle="ProviderPreset 快照、协议 endpoint 与运行期开关彼此独立">
            <TableScroll label="Sources 表格">
              <table className={styles.table}>
                <thead><tr><th>Source</th><th>ProviderPreset</th><th>Base URL</th><th>Chat</th><th>Responses</th><th>Messages</th><th>Accounts</th><th>状态</th><th>操作</th></tr></thead>
                <tbody>{data.sources.map((source) => (
                  <tr key={source.id} data-clickable="true" onClick={() => setSelectedSourceId(source.id)}>
                    <td><span className={styles.primaryText}><strong>{source.display_name}</strong><small className={styles.mono}>{source.id}</small></span></td>
                    <td><code>{source.provider_preset_id}@{source.provider_preset_version}</code></td>
                    <td><code>{source.base_url}</code></td>
                    {(['openai_chat_completions', 'openai_responses', 'anthropic_messages'] as const).map((protocol) => (
                      <td key={protocol}><StatusPill tone={protocolModeTone(source.protocol_capabilities[protocol]?.mode)}>{protocolModeLabel(source.protocol_capabilities[protocol]?.mode)}</StatusPill></td>
                    ))}
                    <td><span className={styles.mono}>{data.accounts.filter((account) => account.source_id === source.id).length}</span></td>
                    <td onClick={(event) => event.stopPropagation()}><Toggle label={`${source.id} 启停`} checked={source.enabled} disabled={mutationBusy} onChange={(enabled) => void mutate(() => api.setSourceEnabled(source.id, enabled), `Source ${source.id} 已${enabled ? '启用' : '停用'}。`)} /></td>
                    <td onClick={(event) => event.stopPropagation()}><div className={styles.rowActions}>
                      <IconButton label={`查看 ${source.id}`} onClick={() => setSelectedSourceId(source.id)}><IconEye size={16} /></IconButton>
                      <IconButton label={`编辑 ${source.id}`} onClick={() => setEditor({ kind: 'source', record: source })}><IconPencil size={16} /></IconButton>
                      <IconButton label={`${source.enabled ? '停用' : '启用'} ${source.id}`} disabled={mutationBusy} onClick={() => void mutate(() => api.setSourceEnabled(source.id, !source.enabled), `Source ${source.id} 已${source.enabled ? '停用' : '启用'}。`)}><IconPower size={16} /></IconButton>
                      <IconButton label={`删除 ${source.id}`} className={styles.dangerIcon} onClick={() => setDeleteTarget({ kind: 'source', record: source })}><IconTrash2 size={16} /></IconButton>
                    </div></td>
                  </tr>
                ))}</tbody>
              </table>
            </TableScroll>
          </Card>
        )
      ) : data.accounts.length === 0 ? <EmptyTable title="尚未配置 Account" description="Account 独立关联到一个 Source。" /> : (
        <Card variant="flush" title="Accounts" subtitle="凭据只以服务端环境变量或密文引用提交，界面不展示秘密">
          <TableScroll label="Accounts 表格">
            <table className={styles.table}>
              <thead><tr><th>Account</th><th>Source</th><th>凭据</th><th>Fallback 权重</th><th>健康状态</th><th>Cooldown</th><th>状态</th><th>操作</th></tr></thead>
              <tbody>{data.accounts.map((account) => (
                <tr key={account.id}>
                  <td><span className={styles.primaryText}><strong>{account.display_name}</strong><small className={styles.mono}>{account.id}</small></span></td>
                  <td><code>{account.source_id}</code></td>
                  <td><StatusPill tone={account.credential_configured ? 'success' : 'danger'}>{credentialLabel(account)}</StatusPill></td>
                  <td><span className={styles.mono}>{account.weight}</span></td>
                  <td><StatusPill tone={account.health_status === 'healthy' ? 'success' : account.health_status === 'unknown' ? 'accent' : 'warning'}>{account.health_status || 'unknown'}</StatusPill></td>
                  <td>{formatDateTime(account.cooldown_until)}</td>
                  <td><Toggle label={`${account.id} 启停`} checked={account.enabled} disabled={mutationBusy} onChange={(enabled) => void mutate(() => api.setAccountEnabled(account.id, enabled), `Account ${account.id} 已${enabled ? '启用' : '停用'}。`)} /></td>
                  <td><div className={styles.rowActions}>
                    <IconButton label={`编辑 ${account.id}`} onClick={() => setEditor({ kind: 'account', record: account })}><IconPencil size={16} /></IconButton>
                    <IconButton label={`${account.enabled ? '停用' : '启用'} ${account.id}`} disabled={mutationBusy} onClick={() => void mutate(() => api.setAccountEnabled(account.id, !account.enabled), `Account ${account.id} 已${account.enabled ? '停用' : '启用'}。`)}><IconPower size={16} /></IconButton>
                    <IconButton label={`删除 ${account.id}`} className={styles.dangerIcon} onClick={() => setDeleteTarget({ kind: 'account', record: account })}><IconTrash2 size={16} /></IconButton>
                  </div></td>
                </tr>
              ))}</tbody>
            </table>
          </TableScroll>
        </Card>
      )}

      {selectedSource && (
        <SourceDetailDrawer
          key={selectedSource.id}
          source={selectedSource}
          accounts={data.accounts.filter((account) => account.source_id === selectedSource.id)}
          api={api}
          onClose={() => setSelectedSourceId(undefined)}
          onEdit={() => {
            setSelectedSourceId(undefined);
            setEditor({ kind: 'source', record: selectedSource });
          }}
        />
      )}

      <Modal
        open={Boolean(editor)}
        title={editor?.kind === 'source'
          ? editor.record ? '编辑 Source' : '新增 Source'
          : editor?.record ? '编辑 Account' : '新增 Account'}
        onClose={() => !mutationBusy && setEditor(undefined)}
        closeDisabled={mutationBusy}
        width={680}
        footer={editor && (
          <>
            <Button variant="secondary" onClick={() => setEditor(undefined)} disabled={mutationBusy}>取消</Button>
            <Button type="submit" form={editor.kind === 'source' ? 'source-editor-form' : 'account-editor-form'} loading={mutationBusy}>
              {editor.record ? '保存更改' : '创建'}
            </Button>
          </>
        )}
      >
        {editor?.kind === 'source' && <SourceForm key={editor.record?.id ?? 'new-source'} record={editor.record} presets={data.presets} busy={mutationBusy} error={mutationError?.message} onSubmit={submitSource} />}
        {editor?.kind === 'account' && <AccountForm key={editor.record?.id ?? 'new-account'} record={editor.record} sources={data.sources} busy={mutationBusy} error={mutationError?.message} onSubmit={submitAccount} />}
      </Modal>

      <ConfirmDialog
        open={Boolean(deleteTarget)}
        title={deleteTarget?.kind === 'source' ? '删除 Source' : '删除 Account'}
        description={deleteTarget ? <div className={styles.page}>确认删除 <code className={styles.mono}>{deleteTarget.record.id}</code>？有关联资源时，后端会拒绝并返回结构化错误。<FormError message={mutationError?.message} /></div> : null}
        confirmLabel="删除"
        danger
        busy={mutationBusy}
        onCancel={() => !mutationBusy && setDeleteTarget(undefined)}
        onConfirm={confirmDelete}
      />
    </section>
  );
}
