import { useCallback, useEffect, useState, type FormEvent } from 'react';
import type {
  Account,
  AdminErrorShape,
  CapabilityMatrixResponse,
  CatalogStatus,
  EffectiveProtocolCapability,
  GatewayAdminResources,
  GatewayProtocol,
  LogicalModel,
  LogicalModelWriteInput,
  MetadataSource,
  ModelBinding,
  ModelBindingWriteInput,
  ModelMetadataField,
  Route,
  RouteWriteInput,
  Source,
  SourceModel,
} from '@/admin-api';
import { GATEWAY_PROTOCOLS, normalizeAdminError } from '@/admin-api';
import { Button } from '@/components/ui/Button';
import { Card } from '@/components/ui/Card';
import { Modal } from '@/components/ui/Modal';
import {
  IconEye,
  IconPencil,
  IconPlus,
  IconPower,
  IconRefreshCw,
  IconTrash2,
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
  TextField,
  Toggle,
  formatDateTime,
} from './shared';
import {
  ModelMetadataFields,
  createMetadataDraft,
  metadataFromDraft,
  type MetadataDraft,
} from './ModelMetadataEditor';
import { useAdminQuery } from './useAdminQuery';
import styles from './ControlPlane.module.scss';

interface ModelsRoutesPageProps {
  api: GatewayAdminResources;
  refreshRevision?: number;
  onBusyChange?: (busy: boolean) => void;
}

interface CatalogData {
  logicalModels: LogicalModel[];
  bindings: ModelBinding[];
  routes: Route[];
  sources: Source[];
  accounts: Account[];
  capabilities: CapabilityMatrixResponse;
}

type CatalogTab = 'logical-models' | 'bindings' | 'routes';
type Editor =
  | { kind: 'logical-model'; record?: LogicalModel }
  | { kind: 'binding'; record?: ModelBinding }
  | { kind: 'route'; record?: Route };
type DeleteTarget =
  | { kind: 'logical-model'; record: LogicalModel }
  | { kind: 'binding'; record: ModelBinding }
  | { kind: 'route'; record: Route };
type DetailTarget = DeleteTarget;

const statusTone = (status: CatalogStatus) => {
  if (status === 'confirmed') return 'success' as const;
  if (status === 'unavailable') return 'danger' as const;
  return 'warning' as const;
};

const statusOptions = (record?: { status: CatalogStatus }): CatalogStatus[] => {
  if (!record || record.status === 'pending') return ['pending', 'confirmed', 'unavailable'];
  if (record.status === 'confirmed') return ['confirmed', 'unavailable'];
  return ['unavailable', 'pending'];
};

interface ResolvedBindingCell {
  routeId: string;
  model: string;
  upstreamModel: string;
  cell: EffectiveProtocolCapability;
}

const resolvedCellsForBinding = (
  capabilities: CapabilityMatrixResponse,
  bindingId: number,
): ResolvedBindingCell[] => capabilities.data.flatMap((row) => row.protocols
  .filter((cell) => cell.binding_id === bindingId && cell.status === 'routable')
  .map((cell) => ({ routeId: row.route_id, model: row.model, upstreamModel: row.upstream_model_id, cell })));

function RuntimeBindingSummary({ cells }: { cells: ResolvedBindingCell[] }) {
  if (cells.length === 0) return <StatusPill tone="muted">not published</StatusPill>;
  return (
    <span className={styles.runtimeSummary}>
      {cells.map(({ routeId, cell }) => (
        <span key={`${routeId}:${cell.protocol_in}`}>
          <StatusPill tone={cell.mode === 'native' ? 'success' : 'warning'}>{cell.mode}</StatusPill>
          <small>{PROTOCOL_LABELS[cell.protocol_in]} → {cell.protocol_upstream ? PROTOCOL_LABELS[cell.protocol_upstream] : 'unknown'}</small>
          <StatusPill tone={cell.selection === 'primary' ? 'accent' : 'muted'}>{cell.selection} #{cell.selection_rank}</StatusPill>
        </span>
      ))}
    </span>
  );
}

function LogicalModelForm({
  record,
  busy,
  error,
  onSubmit,
}: {
  record?: LogicalModel;
  busy: boolean;
  error?: string;
  onSubmit: (input: LogicalModelWriteInput) => void;
}) {
  const [id, setId] = useState(record?.id ?? '');
  const [publicName, setPublicName] = useState(record?.public_name ?? '');
  const [displayName, setDisplayName] = useState(record?.display_name ?? '');
  const [status, setStatus] = useState<CatalogStatus>(record?.status ?? 'pending');
  const [enabled, setEnabled] = useState(record?.enabled ?? true);
  const [draft, setDraft] = useState<MetadataDraft>(() => createMetadataDraft(record?.metadata));
  const [dirtyFields, setDirtyFields] = useState<Set<ModelMetadataField>>(() => new Set());
  const [validationError, setValidationError] = useState('');

  const changeMetadata = (field: ModelMetadataField, value: string) => {
    setDraft((current) => ({ ...current, [field]: value }));
    setDirtyFields((current) => new Set(current).add(field));
  };

  const submit = (event: FormEvent) => {
    event.preventDefault();
    if (!id.trim() || !publicName.trim() || !displayName.trim()) {
      setValidationError('ID、公开模型名和显示名称均为必填项。');
      return;
    }
    const changedMetadata = metadataFromDraft(draft, dirtyFields);
    const metadata = { ...(record?.metadata ?? {}), ...changedMetadata };
    const fieldSources: Partial<Record<ModelMetadataField, MetadataSource>> = {
      ...(record?.field_sources ?? {}),
    };
    for (const field of dirtyFields) fieldSources[field] = 'user';
    const invalidNumber = (['context_window', 'max_input_tokens', 'max_output_tokens'] as const)
      .some((field) => metadata[field] !== undefined && metadata[field] !== null
        && (!Number.isFinite(metadata[field] as number) || (metadata[field] as number) <= 0));
    if (invalidNumber) {
      setValidationError('Token 数值字段必须为空或大于 0。');
      return;
    }
    setValidationError('');
    onSubmit({
      id: id.trim(),
      public_name: publicName.trim(),
      display_name: displayName.trim(),
      status,
      metadata,
      field_sources: fieldSources,
      enabled,
    });
  };

  return (
    <form id="logical-model-editor-form" className={styles.page} onSubmit={submit}>
      <FormGrid>
        <TextField label="LogicalModel ID" value={id} disabled={Boolean(record) || busy} onChange={(event) => setId(event.target.value)} autoComplete="off" />
        <TextField label="公开模型名" value={publicName} disabled={busy} onChange={(event) => setPublicName(event.target.value)} autoComplete="off" />
        <TextField label="显示名称" value={displayName} disabled={busy} onChange={(event) => setDisplayName(event.target.value)} autoComplete="off" />
        <SelectField label="目录状态" value={status} disabled={busy} onChange={(event) => setStatus(event.target.value as CatalogStatus)}>
          {statusOptions(record).map((option) => <option key={option} value={option}>{option}</option>)}
        </SelectField>
        <div className={styles.fullWidth}><CheckboxField checked={enabled} disabled={busy} onChange={setEnabled} label="启用 LogicalModel" /></div>
      </FormGrid>
      <DrawerSection title="模型元数据">
        <ModelMetadataFields draft={draft} fieldSources={record?.field_sources} disabled={busy} onChange={changeMetadata} />
      </DrawerSection>
      <FormError message={validationError || error} />
    </form>
  );
}

function BindingForm({
  record,
  logicalModels,
  sources,
  accounts,
  api,
  busy,
  error,
  onSubmit,
}: {
  record?: ModelBinding;
  logicalModels: LogicalModel[];
  sources: Source[];
  accounts: Account[];
  api: GatewayAdminResources;
  busy: boolean;
  error?: string;
  onSubmit: (input: ModelBindingWriteInput) => void;
}) {
  const [logicalModelId, setLogicalModelId] = useState(record?.logical_model_id ?? logicalModels[0]?.id ?? '');
  const [sourceId, setSourceId] = useState(record?.source_id ?? sources[0]?.id ?? '');
  const availableAccounts = accounts.filter((account) => account.source_id === sourceId);
  const [accountId, setAccountId] = useState(record?.account_id ?? availableAccounts[0]?.id ?? '');
  const [upstreamModelId, setUpstreamModelId] = useState(record?.upstream_model_id ?? '');
  const [protocol, setProtocol] = useState<GatewayProtocol>(record?.protocol ?? 'openai_chat_completions');
  const [status, setStatus] = useState<CatalogStatus>(record?.status ?? 'pending');
  const [priority, setPriority] = useState(record?.priority ?? 100);
  const [enabled, setEnabled] = useState(record?.enabled ?? true);
  const [sourceModels, setSourceModels] = useState<SourceModel[]>([]);
  const [modelsLoading, setModelsLoading] = useState(true);
  const [modelsError, setModelsError] = useState<AdminErrorShape>();
  const [validationError, setValidationError] = useState('');

  const loadModels = useCallback(async (signal: AbortSignal) => {
    if (!sourceId) {
      setSourceModels([]);
      setModelsLoading(false);
      return;
    }
    setModelsLoading(true);
    setModelsError(undefined);
    try {
      const models = await api.sourceModels(sourceId, {}, signal);
      if (!signal.aborted) {
        setSourceModels(models);
        setUpstreamModelId((current) => current || models[0]?.upstream_model_id || '');
      }
    } catch (loadError) {
      if (!signal.aborted) setModelsError(normalizeAdminError(loadError));
    } finally {
      if (!signal.aborted) setModelsLoading(false);
    }
  }, [api, sourceId]);

  useEffect(() => {
    const controller = new AbortController();
    void loadModels(controller.signal);
    return () => controller.abort();
  }, [loadModels]);

  const changeSource = (nextSourceId: string) => {
    setSourceId(nextSourceId);
    setAccountId(accounts.find((account) => account.source_id === nextSourceId)?.id ?? '');
    setUpstreamModelId('');
  };

  const submit = (event: FormEvent) => {
    event.preventDefault();
    if (!logicalModelId || !sourceId || !accountId || !upstreamModelId) {
      setValidationError('LogicalModel、Source、Account 和 SourceModel 均为必填项。');
      return;
    }
    if (!Number.isInteger(priority)) {
      setValidationError('优先级必须是整数。');
      return;
    }
    setValidationError('');
    onSubmit({
      logical_model_id: logicalModelId,
      source_id: sourceId,
      account_id: accountId,
      upstream_model_id: upstreamModelId,
      protocol,
      status,
      enabled,
      priority,
    });
  };

  return (
    <form id="binding-editor-form" className={styles.page} onSubmit={submit}>
      <FormGrid>
        <SelectField label="LogicalModel" value={logicalModelId} disabled={busy} onChange={(event) => setLogicalModelId(event.target.value)}>
          {logicalModels.map((model) => <option key={model.id} value={model.id}>{model.display_name} · {model.id}</option>)}
        </SelectField>
        <SelectField label="入口协议" value={protocol} disabled={busy} onChange={(event) => setProtocol(event.target.value as GatewayProtocol)}>
          {GATEWAY_PROTOCOLS.map((item) => <option key={item} value={item}>{PROTOCOL_LABELS[item]}</option>)}
        </SelectField>
        <SelectField label="Source" value={sourceId} disabled={busy} onChange={(event) => changeSource(event.target.value)}>
          {sources.map((source) => <option key={source.id} value={source.id}>{source.display_name} · {source.id}</option>)}
        </SelectField>
        <SelectField label="Account" value={accountId} disabled={busy || availableAccounts.length === 0} onChange={(event) => setAccountId(event.target.value)}>
          {availableAccounts.length === 0 && <option value="">该 Source 无 Account</option>}
          {availableAccounts.map((account) => <option key={account.id} value={account.id}>{account.display_name} · {account.id}</option>)}
        </SelectField>
        <SelectField label="SourceModel" value={upstreamModelId} disabled={busy || modelsLoading || sourceModels.length === 0} onChange={(event) => setUpstreamModelId(event.target.value)}>
          {modelsLoading && <option value="">正在加载 SourceModel…</option>}
          {!modelsLoading && sourceModels.length === 0 && <option value="">没有 SourceModel</option>}
          {sourceModels.map((model) => <option key={model.upstream_model_id} value={model.upstream_model_id}>{model.upstream_model_id} · {model.confirmation_status}/{model.availability_status}</option>)}
        </SelectField>
        <SelectField label="Binding 状态" value={status} disabled={busy} onChange={(event) => setStatus(event.target.value as CatalogStatus)}>
          {statusOptions(record).map((option) => <option key={option} value={option}>{option}</option>)}
        </SelectField>
        <TextField label="优先级" hint="同模式内数值越高越优先；runtime 仍固定 native 优先。" type="number" step={1} value={priority} disabled={busy} onChange={(event) => setPriority(Number(event.target.value))} />
        <div className={styles.field}><label>Binding ID</label><StatusPill>{record?.id ?? '创建后分配'}</StatusPill></div>
        <div className={styles.fullWidth}><CheckboxField checked={enabled} disabled={busy} onChange={setEnabled} label="启用 ModelBinding" /></div>
      </FormGrid>
      {modelsError && <ErrorState error={modelsError} onRetry={() => void loadModels(new AbortController().signal)} />}
      <FormError message={validationError || error} />
    </form>
  );
}

function RouteForm({
  record,
  logicalModels,
  busy,
  error,
  onSubmit,
}: {
  record?: Route;
  logicalModels: LogicalModel[];
  busy: boolean;
  error?: string;
  onSubmit: (input: RouteWriteInput) => void;
}) {
  const [id, setId] = useState(record?.id ?? '');
  const [logicalModelId, setLogicalModelId] = useState(record?.logical_model_id ?? logicalModels[0]?.id ?? '');
  const [protocols, setProtocols] = useState<Set<GatewayProtocol>>(() => new Set(record?.protocols ?? []));
  const [allowLossy, setAllowLossy] = useState(record?.allow_lossy_conversion ?? false);
  const [enabled, setEnabled] = useState(record?.enabled ?? true);
  const [validationError, setValidationError] = useState('');
  const strategy = 'primary_then_weighted_fallback';

  const submit = (event: FormEvent) => {
    event.preventDefault();
    if (!id.trim() || !logicalModelId || protocols.size === 0) {
      setValidationError('Route ID、LogicalModel 和至少一个入口协议为必填项。');
      return;
    }
    setValidationError('');
    onSubmit({
      id: id.trim(),
      logical_model_id: logicalModelId,
      protocols: GATEWAY_PROTOCOLS.filter((protocol) => protocols.has(protocol)),
      strategy,
      allow_lossy_conversion: allowLossy,
      enabled,
    });
  };

  return (
    <form id="route-editor-form" className={styles.page} onSubmit={submit}>
      <FormGrid>
        <TextField label="Route ID" value={id} disabled={Boolean(record) || busy} onChange={(event) => setId(event.target.value)} autoComplete="off" />
        <SelectField label="LogicalModel" value={logicalModelId} disabled={busy} onChange={(event) => setLogicalModelId(event.target.value)}>
          {logicalModels.map((model) => <option key={model.id} value={model.id}>{model.display_name} · {model.id}</option>)}
        </SelectField>
        <TextField label="路由策略" value={strategy} readOnly disabled />
        <div className={styles.field}><label>选择语义</label><StatusPill tone="accent">fixed primary → fallback</StatusPill></div>
        <div className={styles.fullWidth}>
          <span className={styles.fieldLabel}>入口协议</span>
          <div className={styles.protocolChecks}>{GATEWAY_PROTOCOLS.map((protocol) => (
            <CheckboxField
              key={protocol}
              checked={protocols.has(protocol)}
              disabled={busy}
              label={PROTOCOL_LABELS[protocol]}
              onChange={(checked) => setProtocols((current) => {
                const next = new Set(current);
                if (checked) next.add(protocol); else next.delete(protocol);
                return next;
              })}
            />
          ))}</div>
        </div>
        <CheckboxField checked={allowLossy} disabled={busy} onChange={setAllowLossy} label="允许有损转换" hint="实际损失由有效能力矩阵列出。" />
        <CheckboxField checked={enabled} disabled={busy} onChange={setEnabled} label="启用 Route" />
      </FormGrid>
      <FormError message={validationError || error} />
    </form>
  );
}

function EntityDetailDrawer({
  target,
  data,
  onClose,
  onEdit,
}: {
  target: DetailTarget;
  data: CatalogData;
  onClose: () => void;
  onEdit: () => void;
}) {
  const [open, setOpen] = useState(true);
  useEffect(() => {
    if (open) return;
    const timer = window.setTimeout(onClose, 380);
    return () => window.clearTimeout(timer);
  }, [onClose, open]);
  const title = target.kind === 'logical-model' ? 'LogicalModel 详情' : target.kind === 'binding' ? 'ModelBinding 详情' : 'Route 详情';
  const bindingCells = target.kind === 'binding' ? resolvedCellsForBinding(data.capabilities, target.record.id) : [];
  const routeRows = target.kind === 'route' ? data.capabilities.data.filter((row) => row.route_id === target.record.id) : [];
  return (
    <Modal open={open} variant="drawer" width={600} title={title} onClose={() => setOpen(false)} footer={<><Button variant="secondary" onClick={() => setOpen(false)}>关闭</Button><Button onClick={onEdit}><IconPencil size={14} />编辑</Button></>}>
      <DrawerSection title="领域记录">
        {target.kind === 'logical-model' && <DetailList>
          <DetailItem label="ID"><code>{target.record.id}</code></DetailItem>
          <DetailItem label="Public name"><code>{target.record.public_name}</code></DetailItem>
          <DetailItem label="Display name">{target.record.display_name}</DetailItem>
          <DetailItem label="Status"><StatusPill tone={statusTone(target.record.status)}>{target.record.status}</StatusPill></DetailItem>
          <DetailItem label="Enabled">{String(target.record.enabled)}</DetailItem>
          <DetailItem label="Updated">{formatDateTime(target.record.updated_at)}</DetailItem>
        </DetailList>}
        {target.kind === 'binding' && <DetailList>
          <DetailItem label="Binding ID"><code>{target.record.id}</code></DetailItem>
          <DetailItem label="LogicalModel"><code>{target.record.logical_model_id}</code></DetailItem>
          <DetailItem label="Source / Account"><code>{target.record.source_id} / {target.record.account_id}</code></DetailItem>
          <DetailItem label="Upstream model"><code>{target.record.upstream_model_id}</code></DetailItem>
          <DetailItem label="Protocol"><ProtocolPill protocol={target.record.protocol} /></DetailItem>
          <DetailItem label="Priority">{target.record.priority}</DetailItem>
          <DetailItem label="Status"><StatusPill tone={statusTone(target.record.status)}>{target.record.status}</StatusPill></DetailItem>
          <DetailItem label="Enabled">{String(target.record.enabled)}</DetailItem>
        </DetailList>}
        {target.kind === 'route' && <DetailList>
          <DetailItem label="Route ID"><code>{target.record.id}</code></DetailItem>
          <DetailItem label="LogicalModel"><code>{target.record.logical_model_id}</code></DetailItem>
          <DetailItem label="Public name"><code>{target.record.public_name}</code></DetailItem>
          <DetailItem label="Protocols"><span className={styles.inlineActions}>{target.record.protocols.map((protocol) => <ProtocolPill key={protocol} protocol={protocol} />)}</span></DetailItem>
          <DetailItem label="Strategy"><code>{target.record.strategy}</code></DetailItem>
          <DetailItem label="Lossy">{String(target.record.allow_lossy_conversion)}</DetailItem>
          <DetailItem label="Enabled">{String(target.record.enabled)}</DetailItem>
        </DetailList>}
      </DrawerSection>
      {target.kind === 'binding' && <DrawerSection title="已发布 runtime 解析"><RuntimeBindingSummary cells={bindingCells} /></DrawerSection>}
      {target.kind === 'route' && <DrawerSection title="已发布 Binding">
        {routeRows.length === 0 ? <EmptyTable title="Route 尚未进入 runtime snapshot" /> : (
          <div className={styles.runtimeList}>{routeRows.map((row) => (
            <div key={`${row.source.source_id}:${row.account.account_id}:${row.upstream_model_id}`}>
              <span><strong>{row.source.display_name ?? row.source.source_id}</strong><small>{row.account.display_name ?? row.account.account_id} · {row.upstream_model_id}</small></span>
              <div>{row.protocols.map((cell) => <StatusPill key={cell.protocol_in} tone={cell.status === 'unroutable' ? 'muted' : cell.mode === 'native' ? 'success' : 'warning'}>{PROTOCOL_LABELS[cell.protocol_in]} · {cell.status === 'routable' ? cell.mode : 'unroutable'}</StatusPill>)}</div>
            </div>
          ))}</div>
        )}
      </DrawerSection>}
    </Modal>
  );
}

export function ModelsRoutesPage({ api, refreshRevision = 0, onBusyChange }: ModelsRoutesPageProps) {
  const [tab, setTab] = useState<CatalogTab>('logical-models');
  const [editor, setEditor] = useState<Editor>();
  const [deleteTarget, setDeleteTarget] = useState<DeleteTarget>();
  const [detailTarget, setDetailTarget] = useState<DetailTarget>();
  const [mutationBusy, setMutationBusy] = useState(false);
  const [mutationError, setMutationError] = useState<AdminErrorShape>();
  const [notice, setNotice] = useState('');

  const load = useCallback(async (signal: AbortSignal): Promise<CatalogData> => {
    const [logicalModels, bindings, routes, sources, accounts, capabilities] = await Promise.all([
      api.logicalModels(signal),
      api.modelBindings(signal),
      api.routes(signal),
      api.sources(signal),
      api.accounts(signal),
      api.capabilities(signal),
    ]);
    return { logicalModels, bindings, routes, sources, accounts, capabilities };
  }, [api]);
  const query = useAdminQuery({ load, refreshRevision, onBusyChange });
  const data = query.data;

  const mutate = async (operation: () => Promise<unknown>, successMessage: string) => {
    if (mutationBusy) return;
    setMutationBusy(true);
    setMutationError(undefined);
    onBusyChange?.(true);
    try {
      await operation();
      setEditor(undefined);
      setDeleteTarget(undefined);
      setDetailTarget(undefined);
      setNotice(successMessage);
      query.reload();
    } catch (error) {
      setMutationError(normalizeAdminError(error));
    } finally {
      setMutationBusy(false);
      onBusyChange?.(false);
    }
  };

  const submitLogicalModel = (input: LogicalModelWriteInput) => {
    const record = editor?.kind === 'logical-model' ? editor.record : undefined;
    void mutate(
      () => record ? api.updateLogicalModel(record.id, input) : api.createLogicalModel(input),
      `LogicalModel ${input.id} 已${record ? '更新' : '创建'}。`,
    );
  };
  const submitBinding = (input: ModelBindingWriteInput) => {
    const record = editor?.kind === 'binding' ? editor.record : undefined;
    void mutate(
      () => record ? api.updateModelBinding(record.id, input) : api.createModelBinding(input),
      `ModelBinding ${record?.id ?? ''} 已${record ? '更新' : '创建'}。`,
    );
  };
  const submitRoute = (input: RouteWriteInput) => {
    const record = editor?.kind === 'route' ? editor.record : undefined;
    void mutate(
      () => record ? api.updateRoute(record.id, input) : api.createRoute(input),
      `Route ${input.id} 已${record ? '更新' : '创建'}。`,
    );
  };
  const deleteRecord = () => {
    if (!deleteTarget) return;
    const id = deleteTarget.record.id;
    void mutate(
      () => deleteTarget.kind === 'logical-model'
        ? api.deleteLogicalModel(String(id))
        : deleteTarget.kind === 'binding'
          ? api.deleteModelBinding(Number(id))
          : api.deleteRoute(String(id)),
      `${deleteTarget.kind} ${id} 已删除。`,
    );
  };

  if (query.loading && !data) return <LoadingState label="正在加载 Models & Routes…" />;
  if (query.error && !data) return <ErrorState error={query.error} onRetry={query.reload} />;
  if (!data) return null;

  const sortedBindings = [...data.bindings].sort((left, right) => left.logical_model_id.localeCompare(right.logical_model_id) || left.protocol.localeCompare(right.protocol) || right.priority - left.priority || left.id - right.id);
  const openNew = () => setEditor(tab === 'logical-models' ? { kind: 'logical-model' } : tab === 'bindings' ? { kind: 'binding' } : { kind: 'route' });
  const canCreate = tab === 'logical-models'
    || (tab === 'bindings' && data.logicalModels.length > 0 && data.sources.length > 0 && data.accounts.length > 0)
    || (tab === 'routes' && data.logicalModels.length > 0);

  return (
    <section className={styles.page} data-od-id="page-models-routes">
      <PageActions>
        <SegmentedTabs value={tab} label="模型与路由资源" options={[
          { value: 'logical-models', label: 'Logical Models', count: data.logicalModels.length },
          { value: 'bindings', label: 'Model Bindings', count: data.bindings.length },
          { value: 'routes', label: 'Routes', count: data.routes.length },
        ]} onChange={setTab} />
        <div className={styles.rowActions}>
          <Button variant="secondary" onClick={query.reload} loading={query.refreshing}><IconRefreshCw size={14} />刷新</Button>
          <Button variant="primary" onClick={openNew} disabled={!canCreate}><IconPlus size={14} />{tab === 'logical-models' ? '新增 LogicalModel' : tab === 'bindings' ? '新增 Binding' : '新增 Route'}</Button>
        </div>
      </PageActions>
      <SuccessNotice message={notice} onDismiss={() => setNotice('')} />
      {query.error && <ErrorState error={query.error} onRetry={query.reload} />}
      {mutationError && !editor && !deleteTarget && <ErrorState error={mutationError} />}

      {tab === 'logical-models' && (data.logicalModels.length === 0 ? <EmptyTable title="尚无 LogicalModel" /> : (
        <Card variant="flush" title="Logical Models" subtitle="公开模型目录与 SourceModel、Binding、Route 分离">
          <TableScroll label="LogicalModel 表格"><table className={styles.table}>
            <thead><tr><th>LogicalModel</th><th>Public name</th><th>目录状态</th><th>Bindings</th><th>Routes</th><th>启用</th><th>操作</th></tr></thead>
            <tbody>{data.logicalModels.map((model) => (
              <tr key={model.id} data-clickable="true" onClick={() => setDetailTarget({ kind: 'logical-model', record: model })}>
                <td><span className={styles.primaryText}><strong>{model.display_name}</strong><small><code>{model.id}</code></small></span></td>
                <td><code>{model.public_name}</code></td>
                <td><StatusPill tone={statusTone(model.status)}>{model.status}</StatusPill></td>
                <td>{data.bindings.filter((binding) => binding.logical_model_id === model.id).length}</td>
                <td>{data.routes.filter((route) => route.logical_model_id === model.id).length}</td>
                <td onClick={(event) => event.stopPropagation()}><Toggle label={`${model.id} 启停`} checked={model.enabled} disabled={mutationBusy} onChange={(enabled) => void mutate(() => api.setLogicalModelEnabled(model.id, enabled), `LogicalModel ${model.id} 已${enabled ? '启用' : '停用'}。`)} /></td>
                <td onClick={(event) => event.stopPropagation()}><div className={styles.rowActions}>
                  <IconButton label={`查看 ${model.id}`} onClick={() => setDetailTarget({ kind: 'logical-model', record: model })}><IconEye size={16} /></IconButton>
                  <IconButton label={`编辑 ${model.id}`} onClick={() => setEditor({ kind: 'logical-model', record: model })}><IconPencil size={16} /></IconButton>
                  <IconButton label={`${model.enabled ? '停用' : '启用'} ${model.id}`} onClick={() => void mutate(() => api.setLogicalModelEnabled(model.id, !model.enabled), `LogicalModel ${model.id} 已${model.enabled ? '停用' : '启用'}。`)}><IconPower size={16} /></IconButton>
                  <IconButton label={`删除 ${model.id}`} className={styles.dangerIcon} onClick={() => setDeleteTarget({ kind: 'logical-model', record: model })}><IconTrash2 size={16} /></IconButton>
                </div></td>
              </tr>
            ))}</tbody>
          </table></TableScroll>
        </Card>
      ))}

      {tab === 'bindings' && (data.bindings.length === 0 ? <EmptyTable title="尚无 ModelBinding" description="Binding 显式关联 LogicalModel、Source、Account、SourceModel 和入口协议。" /> : (
        <Card variant="flush" title="Model Bindings" subtitle="同模式按 priority 降序；runtime 固定 native 优先，失败后进入 fallback">
          <TableScroll label="ModelBinding 表格"><table className={`${styles.table} ${styles.bindingsTable}`}>
            <thead><tr><th>Binding</th><th>LogicalModel</th><th>Source / Account</th><th>Upstream model</th><th>Protocol</th><th>Priority</th><th>Runtime</th><th>状态</th><th>启用</th><th>操作</th></tr></thead>
            <tbody>{sortedBindings.map((binding) => {
              const runtimeCells = resolvedCellsForBinding(data.capabilities, binding.id);
              return (
                <tr key={binding.id} data-clickable="true" onClick={() => setDetailTarget({ kind: 'binding', record: binding })}>
                  <td><code>#{binding.id}</code></td>
                  <td><code>{binding.logical_model_id}</code></td>
                  <td><span className={styles.primaryText}><strong>{binding.source_id}</strong><small>{binding.account_id}</small></span></td>
                  <td><code>{binding.upstream_model_id}</code></td>
                  <td><ProtocolPill protocol={binding.protocol} /></td>
                  <td><strong className={styles.mono}>{binding.priority}</strong></td>
                  <td><RuntimeBindingSummary cells={runtimeCells} /></td>
                  <td><StatusPill tone={statusTone(binding.status)}>{binding.status}</StatusPill></td>
                  <td onClick={(event) => event.stopPropagation()}><Toggle label={`Binding ${binding.id} 启停`} checked={binding.enabled} disabled={mutationBusy} onChange={(enabled) => void mutate(() => api.setModelBindingEnabled(binding.id, enabled), `ModelBinding ${binding.id} 已${enabled ? '启用' : '停用'}。`)} /></td>
                  <td onClick={(event) => event.stopPropagation()}><div className={styles.rowActions}>
                    <IconButton label={`查看 Binding ${binding.id}`} onClick={() => setDetailTarget({ kind: 'binding', record: binding })}><IconEye size={16} /></IconButton>
                    <IconButton label={`编辑 Binding ${binding.id}`} onClick={() => setEditor({ kind: 'binding', record: binding })}><IconPencil size={16} /></IconButton>
                    <IconButton label={`${binding.enabled ? '停用' : '启用'} Binding ${binding.id}`} onClick={() => void mutate(() => api.setModelBindingEnabled(binding.id, !binding.enabled), `ModelBinding ${binding.id} 已${binding.enabled ? '停用' : '启用'}。`)}><IconPower size={16} /></IconButton>
                    <IconButton label={`删除 Binding ${binding.id}`} className={styles.dangerIcon} onClick={() => setDeleteTarget({ kind: 'binding', record: binding })}><IconTrash2 size={16} /></IconButton>
                  </div></td>
                </tr>
              );
            })}</tbody>
          </table></TableScroll>
        </Card>
      ))}

      {tab === 'routes' && (data.routes.length === 0 ? <EmptyTable title="尚无 Route" description="Route 只声明 LogicalModel、协议、固定 fallback 策略和 lossy 开关。" /> : (
        <Card variant="flush" title="Routes" subtitle="Source、Account、上游模型与 Adapter 均从 Binding + SourceModelCapability 解析">
          <TableScroll label="Route 表格"><table className={`${styles.table} ${styles.routesTable}`}>
            <thead><tr><th>Route</th><th>LogicalModel</th><th>协议</th><th>策略</th><th>Runtime rows</th><th>Lossy</th><th>启用</th><th>操作</th></tr></thead>
            <tbody>{data.routes.map((route) => {
              const runtimeRows = data.capabilities.data.filter((row) => row.route_id === route.id);
              const adapterCount = runtimeRows.flatMap((row) => row.protocols).filter((cell) => cell.status === 'routable' && cell.mode === 'adapter').length;
              return (
                <tr key={route.id} data-clickable="true" onClick={() => setDetailTarget({ kind: 'route', record: route })}>
                  <td><span className={styles.primaryText}><strong><code>{route.id}</code></strong><small>{route.public_name}</small></span></td>
                  <td><code>{route.logical_model_id}</code></td>
                  <td><span className={styles.inlineActions}>{route.protocols.map((protocol) => <ProtocolPill key={protocol} protocol={protocol} />)}</span></td>
                  <td>{route.strategy === 'primary_then_weighted_fallback'
                    ? <code className={styles.routeStrategy}>{route.strategy}</code>
                    : <StatusPill tone="danger">unsupported: {route.strategy}</StatusPill>}</td>
                  <td><span className={styles.inlineActions}><StatusPill tone={runtimeRows.length > 0 ? 'success' : 'muted'}>{runtimeRows.length > 0 ? `${runtimeRows.length} published` : 'not published'}</StatusPill>{adapterCount > 0 && <StatusPill tone="warning">{adapterCount} adapter cells</StatusPill>}</span></td>
                  <td><StatusPill tone={route.allow_lossy_conversion ? 'warning' : 'muted'}>{route.allow_lossy_conversion ? 'allowed' : 'blocked'}</StatusPill></td>
                  <td onClick={(event) => event.stopPropagation()}><Toggle label={`${route.id} 启停`} checked={route.enabled} disabled={mutationBusy} onChange={(enabled) => void mutate(() => api.setRouteEnabled(route.id, enabled), `Route ${route.id} 已${enabled ? '启用' : '停用'}。`)} /></td>
                  <td onClick={(event) => event.stopPropagation()}><div className={styles.rowActions}>
                    <IconButton label={`查看 ${route.id}`} onClick={() => setDetailTarget({ kind: 'route', record: route })}><IconEye size={16} /></IconButton>
                    <IconButton label={`编辑 ${route.id}`} onClick={() => setEditor({ kind: 'route', record: route })}><IconPencil size={16} /></IconButton>
                    <IconButton label={`${route.enabled ? '停用' : '启用'} ${route.id}`} onClick={() => void mutate(() => api.setRouteEnabled(route.id, !route.enabled), `Route ${route.id} 已${route.enabled ? '停用' : '启用'}。`)}><IconPower size={16} /></IconButton>
                    <IconButton label={`删除 ${route.id}`} className={styles.dangerIcon} onClick={() => setDeleteTarget({ kind: 'route', record: route })}><IconTrash2 size={16} /></IconButton>
                  </div></td>
                </tr>
              );
            })}</tbody>
          </table></TableScroll>
        </Card>
      ))}

      <Modal
        open={Boolean(editor)}
        title={editor?.kind === 'logical-model' ? editor.record ? '编辑 LogicalModel' : '新增 LogicalModel' : editor?.kind === 'binding' ? editor.record ? '编辑 ModelBinding' : '新增 ModelBinding' : editor?.record ? '编辑 Route' : '新增 Route'}
        width={editor?.kind === 'logical-model' ? 780 : 700}
        onClose={() => !mutationBusy && setEditor(undefined)}
        closeDisabled={mutationBusy}
        footer={editor && <><Button variant="secondary" onClick={() => setEditor(undefined)} disabled={mutationBusy}>取消</Button><Button type="submit" form={editor.kind === 'logical-model' ? 'logical-model-editor-form' : editor.kind === 'binding' ? 'binding-editor-form' : 'route-editor-form'} loading={mutationBusy}>保存</Button></>}
      >
        {editor?.kind === 'logical-model' && <LogicalModelForm key={editor.record?.id ?? 'new-logical-model'} record={editor.record} busy={mutationBusy} error={mutationError?.message} onSubmit={submitLogicalModel} />}
        {editor?.kind === 'binding' && <BindingForm key={editor.record?.id ?? 'new-binding'} record={editor.record} logicalModels={data.logicalModels} sources={data.sources} accounts={data.accounts} api={api} busy={mutationBusy} error={mutationError?.message} onSubmit={submitBinding} />}
        {editor?.kind === 'route' && <RouteForm key={editor.record?.id ?? 'new-route'} record={editor.record} logicalModels={data.logicalModels} busy={mutationBusy} error={mutationError?.message} onSubmit={submitRoute} />}
      </Modal>

      <ConfirmDialog
        open={Boolean(deleteTarget)}
        title={`删除 ${deleteTarget?.kind ?? '资源'}`}
        description={deleteTarget ? <div className={styles.page}>确认删除 <code className={styles.mono}>{deleteTarget.record.id}</code>？引用仍存在时，后端会拒绝整个事务。<FormError message={mutationError?.message} /></div> : null}
        confirmLabel="删除"
        danger
        busy={mutationBusy}
        onCancel={() => !mutationBusy && setDeleteTarget(undefined)}
        onConfirm={deleteRecord}
      />

      {detailTarget && <EntityDetailDrawer target={detailTarget} data={data} onClose={() => setDetailTarget(undefined)} onEdit={() => { setEditor(detailTarget); setDetailTarget(undefined); }} />}
    </section>
  );
}
