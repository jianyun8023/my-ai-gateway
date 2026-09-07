import { IconButton } from '@/components/ui/IconButton';
import { LoadingState } from '@/components/ui/LoadingState';
import { SegmentedTabs } from '@/components/ui/SegmentedTabs';
import { SelectField, TextField } from '@/components/ui/FormField';
import { StatusPill } from '@/components/ui/StatusPill';
import { TableScroll } from '@/components/ui/TableScroll';
import { Button } from '@/components/ui/Button';
import { Card } from '@/components/ui/Card';
import { Modal } from '@/components/ui/Modal';
import { useCallback, useEffect, useState, type FormEvent } from 'react';
import { useTranslation } from 'react-i18next';
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
  PROTOCOL_LABELS,
  PageActions,
  ProtocolPill,
  SuccessNotice,
  Toggle,
  formatDateTime,
} from './shared';
import {
  ModelMetadataFields,
  createMetadataDraft,
  metadataFromDraft,
  type MetadataDraft,
} from './ModelMetadataEditor';
import { useLocalizedApiError } from '@/hooks/useLocalizedApiError';
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
  const { t } = useTranslation('console');
  if (cells.length === 0) return <StatusPill tone="muted">{t('models.state.not_published')}</StatusPill>;
  return (
    <span className={styles.runtimeSummary}>
      {cells.map(({ routeId, cell }) => (
        <span key={`${routeId}:${cell.protocol_in}`}>
          <StatusPill tone={cell.mode === 'native' ? 'success' : 'warning'}>{t(`values.mode.${cell.mode}`, { defaultValue: cell.mode })}</StatusPill>
          <small>{PROTOCOL_LABELS[cell.protocol_in]} → {cell.protocol_upstream ? PROTOCOL_LABELS[cell.protocol_upstream] : t('common.unknown')}</small>
          <StatusPill tone={cell.selection === 'primary' ? 'accent' : 'muted'}>{t('models.state.selection', { rank: cell.selection_rank })}</StatusPill>
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
  const { t } = useTranslation('console');
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
      setValidationError(t('models.lm_form.validate_required'));
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
      setValidationError(t('models.lm_form.validate_tokens'));
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
        <TextField label={t('models.field.lm_id')} value={id} disabled={Boolean(record) || busy} onChange={(event) => setId(event.target.value)} autoComplete="off" />
        <TextField label={t('models.field.public_name')} value={publicName} disabled={busy} onChange={(event) => setPublicName(event.target.value)} autoComplete="off" />
        <TextField label={t('models.field.display_name')} value={displayName} disabled={busy} onChange={(event) => setDisplayName(event.target.value)} autoComplete="off" />
        <SelectField label={t('models.field.catalog_status')} value={status} disabled={busy} onChange={(event) => setStatus(event.target.value as CatalogStatus)}>
          {statusOptions(record).map((option) => <option key={option} value={option}>{t(`values.status.${option}`)}</option>)}
        </SelectField>
        <div className={styles.fullWidth}><CheckboxField checked={enabled} disabled={busy} onChange={setEnabled} label={t('models.field.enable_lm')} /></div>
      </FormGrid>
      <DrawerSection title={t('models.field.metadata')}>
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
  const { t } = useTranslation('console');
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
      setValidationError(t('models.binding_form.validate_required'));
      return;
    }
    if (!Number.isInteger(priority)) {
      setValidationError(t('models.binding_form.validate_priority'));
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
        <SelectField label={t('models.field.lm')} value={logicalModelId} disabled={busy} onChange={(event) => setLogicalModelId(event.target.value)}>
          {logicalModels.map((model) => <option key={model.id} value={model.id}>{model.display_name} · {model.id}</option>)}
        </SelectField>
        <SelectField label={t('models.field.protocol_in')} value={protocol} disabled={busy} onChange={(event) => setProtocol(event.target.value as GatewayProtocol)}>
          {GATEWAY_PROTOCOLS.map((item) => <option key={item} value={item}>{PROTOCOL_LABELS[item]}</option>)}
        </SelectField>
        <SelectField label={t('models.field.source')} value={sourceId} disabled={busy} onChange={(event) => changeSource(event.target.value)}>
          {sources.map((source) => <option key={source.id} value={source.id}>{source.display_name} · {source.id}</option>)}
        </SelectField>
        <SelectField label={t('models.field.account')} value={accountId} disabled={busy || availableAccounts.length === 0} onChange={(event) => setAccountId(event.target.value)}>
          {availableAccounts.length === 0 && <option value="">{t('models.binding_form.no_account')}</option>}
          {availableAccounts.map((account) => <option key={account.id} value={account.id}>{account.display_name} · {account.id}</option>)}
        </SelectField>
        <SelectField label={t('models.field.source_model')} value={upstreamModelId} disabled={busy || modelsLoading || sourceModels.length === 0} onChange={(event) => setUpstreamModelId(event.target.value)}>
          {modelsLoading && <option value="">{t('models.binding_form.source_model_loading')}</option>}
          {!modelsLoading && sourceModels.length === 0 && <option value="">{t('models.binding_form.no_source_model')}</option>}
          {sourceModels.map((model) => <option key={model.upstream_model_id} value={model.upstream_model_id}>{model.upstream_model_id} · {t(`values.status.${model.confirmation_status}`, { defaultValue: model.confirmation_status })}/{t(`values.availability.${model.availability_status}`, { defaultValue: model.availability_status })}</option>)}
        </SelectField>
        <SelectField label={t('models.field.binding_status')} value={status} disabled={busy} onChange={(event) => setStatus(event.target.value as CatalogStatus)}>
          {statusOptions(record).map((option) => <option key={option} value={option}>{t(`values.status.${option}`)}</option>)}
        </SelectField>
        <TextField label={t('models.field.priority')} hint={t('models.binding_form.priority_hint')} type="number" step={1} value={priority} disabled={busy} onChange={(event) => setPriority(Number(event.target.value))} />
        <div className={styles.field}><label>{t('models.field.binding_id')}</label><StatusPill>{record?.id ?? t('models.binding_form.binding_id_auto')}</StatusPill></div>
        <div className={styles.fullWidth}><CheckboxField checked={enabled} disabled={busy} onChange={setEnabled} label={t('models.field.enable_binding')} /></div>
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
  const { t } = useTranslation('console');
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
      setValidationError(t('models.route_form.validate_required'));
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
        <TextField label={t('models.field.route_id')} value={id} disabled={Boolean(record) || busy} onChange={(event) => setId(event.target.value)} autoComplete="off" />
        <SelectField label={t('models.field.lm')} value={logicalModelId} disabled={busy} onChange={(event) => setLogicalModelId(event.target.value)}>
          {logicalModels.map((model) => <option key={model.id} value={model.id}>{model.display_name} · {model.id}</option>)}
        </SelectField>
        <TextField label={t('models.field.strategy')} value={strategy} readOnly disabled />
        <div className={styles.field}><label>{t('models.route_form.selection_label')}</label><StatusPill tone="accent">{t('models.state.strategy_fixed')}</StatusPill></div>
        <div className={styles.fullWidth}>
          <span className={styles.fieldLabel}>{t('models.field.protocols')}</span>
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
        <CheckboxField checked={allowLossy} disabled={busy} onChange={setAllowLossy} label={t('models.field.lossy')} hint={t('models.route_form.lossy_hint')} />
        <CheckboxField checked={enabled} disabled={busy} onChange={setEnabled} label={t('models.field.enable_route')} />
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
  const { t } = useTranslation('console');
  const [open, setOpen] = useState(true);
  useEffect(() => {
    if (open) return;
    const timer = window.setTimeout(onClose, 380);
    return () => window.clearTimeout(timer);
  }, [onClose, open]);
  const title = target.kind === 'logical-model' ? t('models.detail.lm_title') : target.kind === 'binding' ? t('models.detail.binding_title') : t('models.detail.route_title');
  const bindingCells = target.kind === 'binding' ? resolvedCellsForBinding(data.capabilities, target.record.id) : [];
  const routeRows = target.kind === 'route' ? data.capabilities.data.filter((row) => row.route_id === target.record.id) : [];
  return (
    <Modal open={open} variant="drawer" width={600} title={title} onClose={() => setOpen(false)} footer={<><Button variant="secondary" onClick={() => setOpen(false)}>{t('common.close')}</Button><Button onClick={onEdit}><IconPencil size={14} />{t('common.edit')}</Button></>}>
      <DrawerSection title={t('models.detail.domain_record')}>
        {target.kind === 'logical-model' && <DetailList>
          <DetailItem label={t('models.field.lm_id')}><code>{target.record.id}</code></DetailItem>
          <DetailItem label={t('models.field.public_name')}><code>{target.record.public_name}</code></DetailItem>
          <DetailItem label={t('models.field.display_name')}>{target.record.display_name}</DetailItem>
          <DetailItem label={t('common.status')}><StatusPill tone={statusTone(target.record.status)}>{t(`values.status.${target.record.status}`, { defaultValue: target.record.status })}</StatusPill></DetailItem>
          <DetailItem label={t('models.field.enabled')}>{String(target.record.enabled)}</DetailItem>
          <DetailItem label={t('common.updated_at')}>{formatDateTime(target.record.updated_at)}</DetailItem>
        </DetailList>}
        {target.kind === 'binding' && <DetailList>
          <DetailItem label={t('models.field.binding_id')}><code>{target.record.id}</code></DetailItem>
          <DetailItem label={t('models.field.lm')}><code>{target.record.logical_model_id}</code></DetailItem>
          <DetailItem label={t('models.field.source_account')}><code>{target.record.source_id} / {target.record.account_id}</code></DetailItem>
          <DetailItem label={t('models.field.upstream_model')}><code>{target.record.upstream_model_id}</code></DetailItem>
          <DetailItem label={t('models.field.protocol')}><ProtocolPill protocol={target.record.protocol} /></DetailItem>
          <DetailItem label={t('models.field.priority')}>{target.record.priority}</DetailItem>
          <DetailItem label={t('common.status')}><StatusPill tone={statusTone(target.record.status)}>{t(`values.status.${target.record.status}`, { defaultValue: target.record.status })}</StatusPill></DetailItem>
          <DetailItem label={t('models.field.enabled')}>{String(target.record.enabled)}</DetailItem>
        </DetailList>}
        {target.kind === 'route' && <DetailList>
          <DetailItem label={t('models.field.route_id')}><code>{target.record.id}</code></DetailItem>
          <DetailItem label={t('models.field.lm')}><code>{target.record.logical_model_id}</code></DetailItem>
          <DetailItem label={t('models.field.public_name')}><code>{target.record.public_name}</code></DetailItem>
          <DetailItem label={t('models.field.protocols')}><span className={styles.inlineActions}>{target.record.protocols.map((protocol) => <ProtocolPill key={protocol} protocol={protocol} />)}</span></DetailItem>
          <DetailItem label={t('models.field.strategy')}><code>{t(`values.strategy.${target.record.strategy}`, { defaultValue: target.record.strategy })}</code></DetailItem>
          <DetailItem label={t('models.field.lossy_value')}>{target.record.allow_lossy_conversion ? t('models.state.lossy_allowed') : t('models.state.lossy_blocked')}</DetailItem>
          <DetailItem label={t('models.field.enabled')}>{String(target.record.enabled)}</DetailItem>
        </DetailList>}
      </DrawerSection>
      {target.kind === 'binding' && <DrawerSection title={t('models.detail.runtime_resolution')}><RuntimeBindingSummary cells={bindingCells} /></DrawerSection>}
      {target.kind === 'route' && <DrawerSection title={t('models.detail.published_binding')}>
        {routeRows.length === 0 ? <EmptyTable title={t('models.detail.not_in_snapshot')} /> : (
          <div className={styles.runtimeList}>{routeRows.map((row) => (
            <div key={`${row.source.source_id}:${row.account.account_id}:${row.upstream_model_id}`}>
              <span><strong>{row.source.display_name ?? row.source.source_id}</strong><small>{row.account.display_name ?? row.account.account_id} · {row.upstream_model_id}</small></span>
              <div>{row.protocols.map((cell) => <StatusPill key={cell.protocol_in} tone={cell.status === 'unroutable' ? 'muted' : cell.mode === 'native' ? 'success' : 'warning'}>{PROTOCOL_LABELS[cell.protocol_in]} · {cell.status === 'routable' ? t(`values.mode.${cell.mode}`, { defaultValue: cell.mode }) : t('models.state.unroutable')}</StatusPill>)}</div>
            </div>
          ))}</div>
        )}
      </DrawerSection>}
    </Modal>
  );
}

export function ModelsRoutesPage({ api, refreshRevision = 0, onBusyChange }: ModelsRoutesPageProps) {
  const { t } = useTranslation('console');
  const apiErrorText = useLocalizedApiError();
  const [tab, setTab] = useState<CatalogTab>('logical-models');
  const [editor, setEditor] = useState<Editor>();
  const [deleteTarget, setDeleteTarget] = useState<DeleteTarget>();
  const [detailTarget, setDetailTarget] = useState<DetailTarget>();
  const [mutationBusy, setMutationBusy] = useState(false);
  const [mutationError, setMutationError] = useState<AdminErrorShape>();
  const [notice, setNotice] = useState('');
  const mutationErrorMessage = mutationError ? apiErrorText(mutationError) : undefined;

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
      record ? t('models.message.lm_updated', { name: input.id }) : t('models.message.lm_created', { name: input.id }),
    );
  };
  const submitBinding = (input: ModelBindingWriteInput) => {
    const record = editor?.kind === 'binding' ? editor.record : undefined;
    void mutate(
      () => record ? api.updateModelBinding(record.id, input) : api.createModelBinding(input),
      record
        ? t('models.message.binding_updated', { name: record.id })
        : t('models.message.binding_created', { name: '' }),
    );
  };
  const submitRoute = (input: RouteWriteInput) => {
    const record = editor?.kind === 'route' ? editor.record : undefined;
    void mutate(
      () => record ? api.updateRoute(record.id, input) : api.createRoute(input),
      record ? t('models.message.route_updated', { name: input.id }) : t('models.message.route_created', { name: input.id }),
    );
  };
  const deleteRecord = () => {
    if (!deleteTarget) return;
    const id = deleteTarget.record.id;
    const deletedMessage = deleteTarget.kind === 'logical-model'
      ? t('models.message.lm_deleted', { name: String(id) })
      : deleteTarget.kind === 'binding'
        ? t('models.message.binding_deleted', { name: String(id) })
        : t('models.message.route_deleted', { name: String(id) });
    void mutate(
      () => deleteTarget.kind === 'logical-model'
        ? api.deleteLogicalModel(String(id))
        : deleteTarget.kind === 'binding'
          ? api.deleteModelBinding(Number(id))
          : api.deleteRoute(String(id)),
      deletedMessage,
    );
  };

  if (query.loading && !data) return <LoadingState label={t('models.loading')} />;
  if (query.error && !data) return <ErrorState error={query.error} onRetry={query.reload} />;
  if (!data) return null;

  const sortedBindings = [...data.bindings].sort((left, right) => left.logical_model_id.localeCompare(right.logical_model_id) || left.protocol.localeCompare(right.protocol) || right.priority - left.priority || left.id - right.id);
  const openNew = () => setEditor(tab === 'logical-models' ? { kind: 'logical-model' } : tab === 'bindings' ? { kind: 'binding' } : { kind: 'route' });
  const canCreate = tab === 'logical-models'
    || (tab === 'bindings' && data.logicalModels.length > 0 && data.sources.length > 0 && data.accounts.length > 0)
    || (tab === 'routes' && data.logicalModels.length > 0);
  const editorTitle = editor?.kind === 'logical-model'
    ? editor.record ? t('models.modal.edit_lm') : t('models.modal.new_lm')
    : editor?.kind === 'binding'
      ? editor.record ? t('models.modal.edit_binding') : t('models.modal.new_binding')
      : editor?.record ? t('models.modal.edit_route') : t('models.modal.new_route');
  const confirmTitle = deleteTarget?.kind === 'logical-model' ? t('models.confirm.delete_lm_title')
    : deleteTarget?.kind === 'binding' ? t('models.confirm.delete_binding_title')
      : deleteTarget?.kind === 'route' ? t('models.confirm.delete_route_title')
        : t('common.delete');

  return (
    <section className={styles.page} data-od-id="page-models-routes">
      <PageActions>
        <SegmentedTabs id="models-tabs" value={tab} label={t('models.region_aria')} options={[
          { value: 'logical-models', label: t('models.tab.logical_models'), count: data.logicalModels.length },
          { value: 'bindings', label: t('models.tab.bindings'), count: data.bindings.length },
          { value: 'routes', label: t('models.tab.routes'), count: data.routes.length },
        ]} onChange={setTab} />
        <div className={styles.rowActions}>
          <Button variant="secondary" onClick={query.reload} loading={query.refreshing}><IconRefreshCw size={14} />{t('common.refresh')}</Button>
          <Button variant="primary" onClick={openNew} disabled={!canCreate}><IconPlus size={14} />{tab === 'logical-models' ? t('models.new.lm') : tab === 'bindings' ? t('models.new.binding') : t('models.new.route')}</Button>
        </div>
      </PageActions>
      <SuccessNotice message={notice} onDismiss={() => setNotice('')} />
      {query.error && <ErrorState error={query.error} onRetry={query.reload} />}
      {mutationError && !editor && !deleteTarget && <ErrorState error={mutationError} />}

      <div role="tabpanel" id="models-tabs-panel" aria-labelledby={`models-tabs-${tab}`} tabIndex={0}>
      {tab === 'logical-models' && (data.logicalModels.length === 0 ? <EmptyTable title={t('models.empty.lm_title')} description={t('models.empty.lm_desc')} /> : (
        <Card variant="flush" title={t('models.card.lm_title')}>
          <TableScroll label={t('models.table.region_logical_models')}><table className={styles.table}>
            <thead><tr><th>{t('models.field.lm')}</th><th>{t('models.field.public_name')}</th><th>{t('models.table.header_catalog_status')}</th><th>{t('models.table.header_bindings')}</th><th>{t('models.table.header_routes')}</th><th>{t('models.table.header_enabled')}</th><th>{t('common.actions')}</th></tr></thead>
            <tbody>{data.logicalModels.map((model) => (
              <tr key={model.id} data-clickable="true" onClick={() => setDetailTarget({ kind: 'logical-model', record: model })}>
                <td><span className={styles.primaryText}><strong>{model.display_name}</strong><small><code>{model.id}</code></small></span></td>
                <td><code>{model.public_name}</code></td>
                <td><StatusPill tone={statusTone(model.status)}>{t(`values.status.${model.status}`, { defaultValue: model.status })}</StatusPill></td>
                <td>{data.bindings.filter((binding) => binding.logical_model_id === model.id).length}</td>
                <td>{data.routes.filter((route) => route.logical_model_id === model.id).length}</td>
                <td onClick={(event) => event.stopPropagation()}><Toggle label={t('models.table.toggle_aria', { id: model.id })} checked={model.enabled} disabled={mutationBusy} onChange={(enabled) => void mutate(() => api.setLogicalModelEnabled(model.id, enabled), t(enabled ? 'models.table.toggle_enabled' : 'models.table.toggle_disabled', { name: model.id }))} /></td>
                <td onClick={(event) => event.stopPropagation()}><div className={styles.rowActions}>
                  <IconButton label={t('models.table.view_aria', { id: model.id })} onClick={() => setDetailTarget({ kind: 'logical-model', record: model })}><IconEye size={16} /></IconButton>
                  <IconButton label={t('models.table.edit_aria', { id: model.id })} onClick={() => setEditor({ kind: 'logical-model', record: model })}><IconPencil size={16} /></IconButton>
                  <IconButton label={model.enabled ? t('models.table.disable_aria', { id: model.id }) : t('models.table.enable_aria', { id: model.id })} onClick={() => void mutate(() => api.setLogicalModelEnabled(model.id, !model.enabled), t(model.enabled ? 'models.table.toggle_disabled' : 'models.table.toggle_enabled', { name: model.id }))}><IconPower size={16} /></IconButton>
                  <IconButton label={t('models.table.delete_aria', { id: model.id })} className={styles.dangerIcon} onClick={() => setDeleteTarget({ kind: 'logical-model', record: model })}><IconTrash2 size={16} /></IconButton>
                </div></td>
              </tr>
            ))}</tbody>
          </table></TableScroll>
        </Card>
      ))}

      {tab === 'bindings' && (data.bindings.length === 0 ? <EmptyTable title={t('models.empty.binding_title')} description={t('models.empty.binding_desc')} /> : (
        <Card variant="flush" title={t('models.card.binding_title')} subtitle={t('models.card.binding_subtitle')}>
          <TableScroll label={t('models.table.region_bindings')}><table className={`${styles.table} ${styles.bindingsTable}`}>
            <thead><tr><th>{t('models.field.binding_id')}</th><th>{t('models.field.lm')}</th><th>{t('models.field.source_account')}</th><th>{t('models.field.upstream_model')}</th><th>{t('models.field.protocol')}</th><th>{t('models.field.priority')}</th><th>{t('models.field.runtime')}</th><th>{t('common.status')}</th><th>{t('models.table.header_enabled')}</th><th>{t('common.actions')}</th></tr></thead>
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
                  <td><StatusPill tone={statusTone(binding.status)}>{t(`values.status.${binding.status}`, { defaultValue: binding.status })}</StatusPill></td>
                  <td onClick={(event) => event.stopPropagation()}><Toggle label={t('models.table.toggle_aria', { id: binding.id })} checked={binding.enabled} disabled={mutationBusy} onChange={(enabled) => void mutate(() => api.setModelBindingEnabled(binding.id, enabled), t(enabled ? 'models.table.binding_toggle_enabled' : 'models.table.binding_toggle_disabled', { name: binding.id }))} /></td>
                  <td onClick={(event) => event.stopPropagation()}><div className={styles.rowActions}>
                    <IconButton label={t('models.table.view_aria', { id: binding.id })} onClick={() => setDetailTarget({ kind: 'binding', record: binding })}><IconEye size={16} /></IconButton>
                    <IconButton label={t('models.table.edit_aria', { id: binding.id })} onClick={() => setEditor({ kind: 'binding', record: binding })}><IconPencil size={16} /></IconButton>
                    <IconButton label={binding.enabled ? t('models.table.disable_aria', { id: binding.id }) : t('models.table.enable_aria', { id: binding.id })} onClick={() => void mutate(() => api.setModelBindingEnabled(binding.id, !binding.enabled), t(binding.enabled ? 'models.table.binding_toggle_disabled' : 'models.table.binding_toggle_enabled', { name: binding.id }))}><IconPower size={16} /></IconButton>
                    <IconButton label={t('models.table.delete_aria', { id: binding.id })} className={styles.dangerIcon} onClick={() => setDeleteTarget({ kind: 'binding', record: binding })}><IconTrash2 size={16} /></IconButton>
                  </div></td>
                </tr>
              );
            })}</tbody>
          </table></TableScroll>
        </Card>
      ))}

      {tab === 'routes' && (data.routes.length === 0 ? <EmptyTable title={t('models.empty.route_title')} description={t('models.empty.route_desc')} /> : (
        <Card variant="flush" title={t('models.card.route_title')}>
          <TableScroll label={t('models.table.region_routes')}><table className={`${styles.table} ${styles.routesTable}`}>
            <thead><tr><th>{t('models.field.route_id')}</th><th>{t('models.field.lm')}</th><th>{t('models.field.protocols')}</th><th>{t('models.field.strategy')}</th><th>{t('models.table.header_runtime_rows')}</th><th>{t('models.field.lossy_value')}</th><th>{t('models.table.header_enabled')}</th><th>{t('common.actions')}</th></tr></thead>
            <tbody>{data.routes.map((route) => {
              const runtimeRows = data.capabilities.data.filter((row) => row.route_id === route.id);
              const adapterCount = runtimeRows.flatMap((row) => row.protocols).filter((cell) => cell.status === 'routable' && cell.mode === 'adapter').length;
              return (
                <tr key={route.id} data-clickable="true" onClick={() => setDetailTarget({ kind: 'route', record: route })}>
                  <td><span className={styles.primaryText}><strong><code>{route.id}</code></strong><small>{route.public_name}</small></span></td>
                  <td><code>{route.logical_model_id}</code></td>
                  <td><span className={styles.inlineActions}>{route.protocols.map((protocol) => <ProtocolPill key={protocol} protocol={protocol} />)}</span></td>
                  <td>{route.strategy === 'primary_then_weighted_fallback'
                    ? <code className={styles.routeStrategy} title={route.strategy}>{t('values.strategy.primary_then_weighted_fallback')}</code>
                    : <StatusPill tone="danger">{t('models.table.runtime_unsupported', { strategy: route.strategy })}</StatusPill>}</td>
                  <td><span className={styles.inlineActions}><StatusPill tone={runtimeRows.length > 0 ? 'success' : 'muted'}>{runtimeRows.length > 0 ? `${runtimeRows.length} ${t('models.state.published')}` : t('models.state.not_published')}</StatusPill>{adapterCount > 0 && <StatusPill tone="warning">{t('models.table.runtime_adapter_cells', { count: adapterCount })}</StatusPill>}</span></td>
                  <td><StatusPill tone={route.allow_lossy_conversion ? 'warning' : 'muted'}>{route.allow_lossy_conversion ? t('models.state.lossy_allowed') : t('models.state.lossy_blocked')}</StatusPill></td>
                  <td onClick={(event) => event.stopPropagation()}><Toggle label={t('models.table.toggle_aria', { id: route.id })} checked={route.enabled} disabled={mutationBusy} onChange={(enabled) => void mutate(() => api.setRouteEnabled(route.id, enabled), t(enabled ? 'models.table.route_toggle_enabled' : 'models.table.route_toggle_disabled', { name: route.id }))} /></td>
                  <td onClick={(event) => event.stopPropagation()}><div className={styles.rowActions}>
                    <IconButton label={t('models.table.view_aria', { id: route.id })} onClick={() => setDetailTarget({ kind: 'route', record: route })}><IconEye size={16} /></IconButton>
                    <IconButton label={t('models.table.edit_aria', { id: route.id })} onClick={() => setEditor({ kind: 'route', record: route })}><IconPencil size={16} /></IconButton>
                    <IconButton label={route.enabled ? t('models.table.disable_aria', { id: route.id }) : t('models.table.enable_aria', { id: route.id })} onClick={() => void mutate(() => api.setRouteEnabled(route.id, !route.enabled), t(route.enabled ? 'models.table.route_toggle_disabled' : 'models.table.route_toggle_enabled', { name: route.id }))}><IconPower size={16} /></IconButton>
                    <IconButton label={t('models.table.delete_aria', { id: route.id })} className={styles.dangerIcon} onClick={() => setDeleteTarget({ kind: 'route', record: route })}><IconTrash2 size={16} /></IconButton>
                  </div></td>
                </tr>
              );
            })}</tbody>
          </table></TableScroll>
        </Card>
      ))}

      </div>
      <Modal
        open={Boolean(editor)}
        title={editorTitle}
        width={editor?.kind === 'logical-model' ? 780 : 700}
        onClose={() => !mutationBusy && setEditor(undefined)}
        closeDisabled={mutationBusy}
        footer={editor && <><Button variant="secondary" onClick={() => setEditor(undefined)} disabled={mutationBusy}>{t('common.cancel')}</Button><Button type="submit" form={editor.kind === 'logical-model' ? 'logical-model-editor-form' : editor.kind === 'binding' ? 'binding-editor-form' : 'route-editor-form'} loading={mutationBusy}>{t('common.save')}</Button></>}
      >
        {editor?.kind === 'logical-model' && <LogicalModelForm key={editor.record?.id ?? 'new-logical-model'} record={editor.record} busy={mutationBusy} error={mutationErrorMessage} onSubmit={submitLogicalModel} />}
        {editor?.kind === 'binding' && <BindingForm key={editor.record?.id ?? 'new-binding'} record={editor.record} logicalModels={data.logicalModels} sources={data.sources} accounts={data.accounts} api={api} busy={mutationBusy} error={mutationErrorMessage} onSubmit={submitBinding} />}
        {editor?.kind === 'route' && <RouteForm key={editor.record?.id ?? 'new-route'} record={editor.record} logicalModels={data.logicalModels} busy={mutationBusy} error={mutationErrorMessage} onSubmit={submitRoute} />}
      </Modal>

      <ConfirmDialog
        open={Boolean(deleteTarget)}
        title={confirmTitle}
        description={deleteTarget ? <div className={styles.page}>{t('models.confirm.delete_body', { id: deleteTarget.record.id })}<FormError message={mutationErrorMessage} /></div> : null}
        confirmLabel={t('common.delete')}
        danger
        busy={mutationBusy}
        onCancel={() => !mutationBusy && setDeleteTarget(undefined)}
        onConfirm={deleteRecord}
      />

      {detailTarget && <EntityDetailDrawer target={detailTarget} data={data} onClose={() => setDetailTarget(undefined)} onEdit={() => { setEditor(detailTarget); setDetailTarget(undefined); }} />}
    </section>
  );
}
