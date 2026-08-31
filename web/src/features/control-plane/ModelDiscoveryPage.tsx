import { useCallback, useMemo, useState, type FormEvent } from 'react';
import type {
  Account,
  AdminErrorShape,
  CatalogAvailability,
  CatalogStatus,
  DiscoveryDiff,
  GatewayAdminResources,
  LatestDiscovery,
  ModelMetadataField,
  ModelMetadataValues,
  ProviderPresetDefinition,
  Source,
  SourceModel,
} from '@/admin-api';
import { normalizeAdminError } from '@/admin-api';
import { Button } from '@/components/ui/Button';
import { Card } from '@/components/ui/Card';
import { Modal } from '@/components/ui/Modal';
import {
  IconCircleCheck,
  IconPencil,
  IconPlay,
  IconRefreshCw,
  IconTriangleAlert,
} from '@/components/ui/icons';
import {
  ConfirmDialog,
  EmptyTable,
  ErrorState,
  FilterBar,
  FormError,
  LoadingState,
  PageActions,
  SelectField,
  StatusPill,
  SuccessNotice,
  TableScroll,
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

interface ModelDiscoveryPageProps {
  api: GatewayAdminResources;
  refreshRevision?: number;
  onBusyChange?: (busy: boolean) => void;
}

interface DiscoveryContext {
  sources: Source[];
  accounts: Account[];
}

interface DiscoveryView {
  latest: LatestDiscovery | null;
  models: SourceModel[];
}

const emptyDiff = (): DiscoveryDiff => ({ added: [], changed: [], missing: [] });

const sourceDiscoveryDefinition = (source?: Source) => {
  if (!source?.provider_preset_snapshot || typeof source.provider_preset_snapshot !== 'object') return undefined;
  const definition = source.provider_preset_snapshot as Partial<ProviderPresetDefinition>;
  return definition.discovery;
};

const statusTone = (status: string) => {
  if (status === 'succeeded' || status === 'confirmed' || status === 'available') return 'success' as const;
  if (status === 'failed' || status === 'unavailable') return 'danger' as const;
  if (status === 'unsupported' || status === 'pending') return 'warning' as const;
  return 'accent' as const;
};

const metadataSummary = (model: SourceModel): string => {
  const displayName = typeof model.metadata.display_name === 'string' ? model.metadata.display_name : '';
  const logicalName = typeof model.metadata.logical_model_name === 'string' ? model.metadata.logical_model_name : '';
  return displayName || logicalName || 'metadata 未命名';
};

const metadataSourcesSummary = (model: SourceModel): string => {
  const counts = new Map<string, number>();
  for (const source of Object.values(model.field_sources)) {
    if (source) counts.set(source, (counts.get(source) ?? 0) + 1);
  }
  return [...counts.entries()].map(([source, count]) => `${source} ${count}`).join(' · ') || 'unknown';
};

function DiffColumn({
  title,
  tone,
  entries,
}: {
  title: string;
  tone: 'success' | 'warning' | 'danger';
  entries: DiscoveryDiff['added'];
}) {
  return (
    <section className={styles.diffColumn}>
      <header><h3>{title}</h3><StatusPill tone={tone}>{entries.length}</StatusPill></header>
      {entries.length === 0 ? <span>无变化</span> : (
        <ul>{entries.map((entry) => (
          <li key={entry.upstream_model_id}>
            <code>{entry.upstream_model_id}</code>
            {entry.changed_fields.length > 0 && <small>{entry.changed_fields.join(', ')}</small>}
          </li>
        ))}</ul>
      )}
    </section>
  );
}

function LatestRunPanel({ latest }: { latest: LatestDiscovery | null }) {
  if (!latest) return <EmptyTable title="尚无 discovery run" description="运行发现后，这里会显示审计状态与稳定 diff。" />;
  const { run } = latest;
  const diff = latest.diff ?? run.diff ?? emptyDiff();
  return (
    <div className={styles.page}>
      <div className={styles.runHeader}>
        <span className={styles.primaryText}>
          <strong><StatusPill tone={statusTone(run.status)}>{run.status}</StatusPill> run #{run.id}</strong>
          <small>{formatDateTime(run.completed_at)} · {run.latency_ms} ms · {run.discovered_model_count} models</small>
        </span>
        <span className={styles.primaryText}>
          <strong>{run.provider_preset_id}@{run.provider_preset_version}</strong>
          <small>Account {run.account_id ?? 'none'} · HTTP {run.http_status ?? 'none'}</small>
        </span>
      </div>
      {run.status === 'unsupported' && (
        <div className={styles.warningState} role="status"><IconTriangleAlert size={17} /><span><strong>该 ProviderPreset 明确不支持模型发现</strong>{run.error_message && <small>{run.error_code}: {run.error_message}</small>}</span></div>
      )}
      {run.status === 'failed' && (
        <div className={styles.errorState} role="alert"><IconTriangleAlert size={17} /><div><strong>模型发现失败</strong><span>{run.error_message ?? '上游发现未成功'}</span>{run.error_code && <code>{run.error_code}</code>}</div></div>
      )}
      {run.status === 'succeeded' && run.discovered_model_count === 0 && (
        <EmptyTable title="发现成功，但上游返回空模型列表" description="现有 SourceModel 未被虚构或自动删除。" />
      )}
      <div className={styles.diffGrid}>
        <DiffColumn title="Added" tone="success" entries={diff.added} />
        <DiffColumn title="Changed" tone="warning" entries={diff.changed} />
        <DiffColumn title="Missing" tone="danger" entries={diff.missing} />
      </div>
    </div>
  );
}

function SourceModelEditor({
  model,
  busy,
  error,
  onSubmit,
}: {
  model: SourceModel;
  busy: boolean;
  error?: string;
  onSubmit: (metadata: ModelMetadataValues) => void;
}) {
  const [draft, setDraft] = useState<MetadataDraft>(() => createMetadataDraft(model.metadata));
  const [dirty, setDirty] = useState<Set<ModelMetadataField>>(() => new Set());
  const [validationError, setValidationError] = useState('');

  const change = (field: ModelMetadataField, value: string) => {
    setDraft((current) => ({ ...current, [field]: value }));
    setDirty((current) => new Set(current).add(field));
  };

  const submit = (event: FormEvent) => {
    event.preventDefault();
    if (dirty.size === 0) {
      setValidationError('没有需要保存的字段变更。');
      return;
    }
    const metadata = metadataFromDraft(draft, dirty);
    const invalidNumber = (['context_window', 'max_input_tokens', 'max_output_tokens'] as const)
      .some((field) => metadata[field] !== undefined && metadata[field] !== null
        && (!Number.isFinite(metadata[field] as number) || (metadata[field] as number) <= 0));
    if (invalidNumber) {
      setValidationError('Token 数值字段必须为空或大于 0。');
      return;
    }
    setValidationError('');
    onSubmit(metadata);
  };

  return (
    <form id="source-model-editor-form" className={styles.page} onSubmit={submit}>
      <div className={styles.modelIdentity}>
        <code>{model.upstream_model_id}</code>
        <span><StatusPill tone={statusTone(model.confirmation_status)}>{model.confirmation_status}</StatusPill><StatusPill tone={statusTone(model.availability_status)}>{model.availability_status}</StatusPill></span>
      </div>
      <ModelMetadataFields draft={draft} fieldSources={model.field_sources} disabled={busy} onChange={change} />
      <FormError message={validationError || error} />
    </form>
  );
}

export function ModelDiscoveryPage({ api, refreshRevision = 0, onBusyChange }: ModelDiscoveryPageProps) {
  const [sourceId, setSourceId] = useState('');
  const [accountId, setAccountId] = useState('');
  const [confirmationFilter, setConfirmationFilter] = useState<CatalogStatus | ''>('pending');
  const [availabilityFilter, setAvailabilityFilter] = useState<CatalogAvailability | ''>('');
  const [selectedModels, setSelectedModels] = useState<Set<string>>(() => new Set());
  const [editingModel, setEditingModel] = useState<SourceModel>();
  const [confirmOpen, setConfirmOpen] = useState(false);
  const [mutationBusy, setMutationBusy] = useState(false);
  const [mutationError, setMutationError] = useState<AdminErrorShape>();
  const [notice, setNotice] = useState('');

  const loadContext = useCallback(async (signal: AbortSignal): Promise<DiscoveryContext> => {
    const [sources, accounts] = await Promise.all([api.sources(signal), api.accounts(signal)]);
    return { sources, accounts };
  }, [api]);
  const contextQuery = useAdminQuery({ load: loadContext, refreshRevision, onBusyChange });
  const context = contextQuery.data;
  const effectiveSourceId = context?.sources.some((item) => item.id === sourceId)
    ? sourceId
    : context?.sources[0]?.id || '';
  const source = context?.sources.find((item) => item.id === effectiveSourceId);
  const enabledAccounts = useMemo(
    () => (context?.accounts ?? []).filter((account) => account.source_id === effectiveSourceId && account.enabled),
    [context?.accounts, effectiveSourceId],
  );
  const effectiveAccountId = enabledAccounts.some((account) => account.id === accountId)
    ? accountId
    : enabledAccounts[0]?.id ?? '';

  const loadDiscovery = useCallback(async (signal: AbortSignal): Promise<DiscoveryView> => {
    if (!effectiveSourceId) return { latest: null, models: [] };
    const [latest, models] = await Promise.all([
      api.latestDiscovery(effectiveSourceId, signal),
      api.sourceModels(effectiveSourceId, {
        confirmationStatus: confirmationFilter || undefined,
        availabilityStatus: availabilityFilter || undefined,
      }, signal),
    ]);
    return { latest, models };
  }, [api, availabilityFilter, confirmationFilter, effectiveSourceId]);
  const discoveryQuery = useAdminQuery({ load: loadDiscovery, refreshRevision });
  const discovery = discoveryQuery.data;
  const visibleModels = (discovery?.models ?? []).filter((model) => model.source_id === effectiveSourceId);
  const eligibleModels = visibleModels.filter((model) => model.confirmation_status === 'pending' && model.availability_status === 'available');
  const discoveryDefinition = sourceDiscoveryDefinition(source);

  const changeSource = (nextSourceId: string) => {
    setSourceId(nextSourceId);
    setAccountId('');
    setSelectedModels(new Set());
  };

  const runMutation = async <T,>(
    operation: () => Promise<T>,
    successMessage: string | ((result: T) => string),
  ) => {
    if (mutationBusy) return;
    setMutationBusy(true);
    setMutationError(undefined);
    onBusyChange?.(true);
    try {
      const result = await operation();
      setNotice(typeof successMessage === 'function' ? successMessage(result) : successMessage);
      setEditingModel(undefined);
      setConfirmOpen(false);
      setSelectedModels(new Set());
      discoveryQuery.reload();
    } catch (error) {
      setMutationError(normalizeAdminError(error));
    } finally {
      setMutationBusy(false);
      onBusyChange?.(false);
    }
  };

  const runDiscovery = () => {
    if (!effectiveSourceId || !effectiveAccountId) return;
    void runMutation(
      () => api.runDiscovery(effectiveSourceId, effectiveAccountId),
      (result) => result.run.status === 'unsupported'
        ? `Discovery unsupported: ${result.run.error_code ?? 'discovery_unsupported'}`
        : result.run.status === 'failed'
          ? `Discovery 已记录失败 run #${result.run.id}。`
          : `Discovery run #${result.run.id} 完成，发现 ${result.run.discovered_model_count} 个模型。`,
    );
  };

  const saveModel = (metadata: ModelMetadataValues) => {
    if (!editingModel) return;
    void runMutation(
      () => api.editSourceModel(editingModel.source_id, {
        upstream_model_id: editingModel.upstream_model_id,
        metadata,
      }),
      `SourceModel ${editingModel.upstream_model_id} 的用户字段已保存。`,
    );
  };

  const confirmSelected = () => {
    const models = eligibleModels
      .filter((model) => selectedModels.has(model.upstream_model_id))
      .map((model) => ({ upstream_model_id: model.upstream_model_id, metadata: {} }));
    if (!effectiveSourceId || models.length === 0) return;
    void runMutation(
      () => api.confirmSourceModels(effectiveSourceId, models),
      `已确认 ${models.length} 个 SourceModel。`,
    );
  };

  const toggleAll = (checked: boolean) => {
    setSelectedModels(checked ? new Set(eligibleModels.map((model) => model.upstream_model_id)) : new Set());
  };

  if (contextQuery.loading && !context) return <LoadingState label="正在加载发现上下文…" />;
  if (contextQuery.error && !context) return <ErrorState error={contextQuery.error} onRetry={contextQuery.reload} />;
  if (!context) return null;
  if (context.sources.length === 0) return <EmptyTable title="没有可用于模型发现的 Source" description="先在 Sources 页面创建 Source 和 Account。" />;

  return (
    <section className={styles.page} data-od-id="page-model-discovery">
      <PageActions>
        <div className={styles.inlineActions}>
          <SelectField label="Source" value={effectiveSourceId} onChange={(event) => changeSource(event.target.value)}>
            {context.sources.map((item) => <option key={item.id} value={item.id}>{item.display_name} · {item.id}</option>)}
          </SelectField>
          <SelectField label="Account" value={effectiveAccountId} disabled={enabledAccounts.length === 0} onChange={(event) => setAccountId(event.target.value)}>
            {enabledAccounts.length === 0 && <option value="">无启用 Account</option>}
            {enabledAccounts.map((account) => <option key={account.id} value={account.id}>{account.display_name} · {account.id}</option>)}
          </SelectField>
        </div>
        <div className={styles.rowActions}>
          <Button variant="secondary" onClick={() => { contextQuery.reload(); discoveryQuery.reload(); }} loading={contextQuery.refreshing || discoveryQuery.refreshing}><IconRefreshCw size={14} />刷新</Button>
          <Button variant="primary" onClick={runDiscovery} loading={mutationBusy} disabled={!effectiveAccountId}>
            <IconPlay size={14} />运行发现
          </Button>
        </div>
      </PageActions>

      <SuccessNotice message={notice} onDismiss={() => setNotice('')} />
      {contextQuery.error && <ErrorState error={contextQuery.error} onRetry={contextQuery.reload} />}
      {mutationError && !editingModel && !confirmOpen && <ErrorState error={mutationError} />}

      {discoveryDefinition?.support === 'unsupported' && !discovery?.latest && (
        <div className={styles.warningState} role="status">
          <IconTriangleAlert size={17} />
          <span><strong>ProviderPreset 将 discovery 声明为 unsupported</strong><small>{discoveryDefinition.reason}</small></span>
        </div>
      )}

      <Card title="Latest run" subtitle="审计 run、上游结果与 added / changed / missing 差异" extra={<StatusPill tone="accent">{source?.provider_preset_id}@{source?.provider_preset_version}</StatusPill>}>
        {discoveryQuery.loading && !discovery ? <LoadingState label="正在加载 latest run…" /> : discoveryQuery.error ? <ErrorState error={discoveryQuery.error} onRetry={discoveryQuery.reload} /> : <LatestRunPanel latest={discovery?.latest?.run.source_id === effectiveSourceId ? discovery.latest : null} />}
      </Card>

      <FilterBar>
        <label>确认状态<select value={confirmationFilter} onChange={(event) => { setConfirmationFilter(event.target.value as CatalogStatus | ''); setSelectedModels(new Set()); }}><option value="">全部</option><option value="pending">pending</option><option value="confirmed">confirmed</option><option value="unavailable">unavailable</option></select></label>
        <label>可用状态<select value={availabilityFilter} onChange={(event) => { setAvailabilityFilter(event.target.value as CatalogAvailability | ''); setSelectedModels(new Set()); }}><option value="">全部</option><option value="unknown">unknown</option><option value="available">available</option><option value="unavailable">unavailable</option></select></label>
        <span className={styles.filterMeta}>{visibleModels.length} 个 SourceModel</span>
      </FilterBar>

      {discoveryQuery.loading && !discovery ? <LoadingState label="正在加载 SourceModel…" /> : visibleModels.length === 0 ? (
        <EmptyTable
          title={discovery?.latest?.run.status === 'unsupported' ? '该 Source 不支持自动发现' : '当前筛选没有 SourceModel'}
          description={discovery?.latest?.run.status === 'failed' ? '最近一次发现失败，既有 SourceModel 未被修改。' : '运行发现或调整筛选条件。'}
        />
      ) : (
        <Card variant="flush" title="SourceModels" subtitle="确认只改变 SourceModel；不会创建 LogicalModel、Binding 或 Route" extra={(
          <div className={styles.rowActions}>
            <StatusPill tone="warning">已选 {selectedModels.size}</StatusPill>
            <Button size="sm" variant="secondary" onClick={() => setConfirmOpen(true)} disabled={selectedModels.size === 0 || mutationBusy}><IconCircleCheck size={14} />批量确认</Button>
          </div>
        )}>
          <TableScroll label="SourceModel 表格">
            <table className={styles.table}>
              <thead><tr><th><label className={styles.tableCheckbox}><input aria-label="选择全部可确认模型" type="checkbox" checked={eligibleModels.length > 0 && eligibleModels.every((model) => selectedModels.has(model.upstream_model_id))} onChange={(event) => toggleAll(event.target.checked)} /><span aria-hidden="true" /></label></th><th>Upstream model</th><th>确认</th><th>可用性</th><th>Metadata</th><th>字段来源</th><th>预设匹配</th><th>最近发现</th><th>操作</th></tr></thead>
              <tbody>{visibleModels.map((model) => {
                const eligible = model.confirmation_status === 'pending' && model.availability_status === 'available';
                return (
                  <tr key={`${model.source_id}:${model.upstream_model_id}`}>
                    <td><label className={styles.tableCheckbox}><input aria-label={`选择 ${model.upstream_model_id}`} type="checkbox" disabled={!eligible} checked={selectedModels.has(model.upstream_model_id)} onChange={(event) => setSelectedModels((current) => { const next = new Set(current); if (event.target.checked) next.add(model.upstream_model_id); else next.delete(model.upstream_model_id); return next; })} /><span aria-hidden="true" /></label></td>
                    <td><code>{model.upstream_model_id}</code></td>
                    <td><StatusPill tone={statusTone(model.confirmation_status)}>{model.confirmation_status}</StatusPill></td>
                    <td><StatusPill tone={statusTone(model.availability_status)}>{model.availability_status}</StatusPill></td>
                    <td><span className={styles.primaryText}><strong>{metadataSummary(model)}</strong><small>context {String(model.metadata.context_window ?? 'unknown')}</small></span></td>
                    <td><span className={styles.secondaryText}>{metadataSourcesSummary(model)}</span></td>
                    <td>{model.matched_model_preset_id ? <code>{model.matched_model_preset_id}@{model.matched_model_preset_version}</code> : <StatusPill>none</StatusPill>}</td>
                    <td>{formatDateTime(model.last_discovered_at)}</td>
                    <td><Button size="sm" variant="ghost" onClick={() => setEditingModel(model)} disabled={model.confirmation_status !== 'pending'}><IconPencil size={14} />编辑</Button></td>
                  </tr>
                );
              })}</tbody>
            </table>
          </TableScroll>
        </Card>
      )}

      <Modal
        open={Boolean(editingModel)}
        title="编辑 pending SourceModel"
        width={760}
        onClose={() => !mutationBusy && setEditingModel(undefined)}
        closeDisabled={mutationBusy}
        footer={(
          <>
            <Button variant="secondary" onClick={() => setEditingModel(undefined)} disabled={mutationBusy}>取消</Button>
            <Button type="submit" form="source-model-editor-form" loading={mutationBusy}><IconPencil size={14} />保存用户字段</Button>
          </>
        )}
      >
        {editingModel && <SourceModelEditor key={`${editingModel.source_id}:${editingModel.upstream_model_id}`} model={editingModel} busy={mutationBusy} error={mutationError?.message} onSubmit={saveModel} />}
      </Modal>

      <ConfirmDialog
        open={confirmOpen}
        title="批量确认 SourceModel"
        description={<>将确认当前选择的 {selectedModels.size} 个可用 pending SourceModel。此操作不会隐式创建 LogicalModel、Binding 或 Route。</>}
        confirmLabel="确认模型"
        busy={mutationBusy}
        onCancel={() => !mutationBusy && setConfirmOpen(false)}
        onConfirm={confirmSelected}
      />
    </section>
  );
}
