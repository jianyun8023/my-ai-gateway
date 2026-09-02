import { useCallback, useMemo, useState, type FormEvent } from 'react';
import { useTranslation } from 'react-i18next';
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
import { useLocalizedApiError } from '@/hooks/useLocalizedApiError';
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
  return displayName || logicalName;
};

const metadataSourcesSummary = (model: SourceModel, sourceLabel: (source: string) => string): string => {
  const counts = new Map<string, number>();
  for (const source of Object.values(model.field_sources)) {
    if (source) counts.set(source, (counts.get(source) ?? 0) + 1);
  }
  const parts = [...counts.entries()].map(([source, count]) => `${sourceLabel(source)} ${count}`);
  return parts.length > 0 ? parts.join(' · ') : sourceLabel('unknown');
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
  const { t } = useTranslation('console');
  return (
    <section className={styles.diffColumn}>
      <header><h3>{title}</h3><StatusPill tone={tone}>{entries.length}</StatusPill></header>
      {entries.length === 0 ? <span>{t('discovery.diff_none')}</span> : (
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
  const { t } = useTranslation('console');
  if (!latest) return <EmptyTable title={t('discovery.empty_run_title')} description={t('discovery.empty_run_desc')} />;
  const { run } = latest;
  const diff = latest.diff ?? run.diff ?? emptyDiff();
  return (
    <div className={styles.page}>
      <div className={styles.runHeader}>
        <span className={styles.primaryText}>
          <strong><StatusPill tone={statusTone(run.status)}>{run.status}</StatusPill> {t('discovery.run_badge', { id: run.id })}</strong>
          <small>{formatDateTime(run.completed_at)} · {t('discovery.run_meta', { duration: run.latency_ms, count: run.discovered_model_count })}</small>
        </span>
        <span className={styles.primaryText}>
          <strong>{run.provider_preset_id}@{run.provider_preset_version}</strong>
          <small>{t('discovery.account_http', { account: run.account_id ?? t('discovery.none'), http: run.http_status ?? t('discovery.none') })}</small>
        </span>
      </div>
      {run.status === 'unsupported' && (
        <div className={styles.warningState} role="status"><IconTriangleAlert size={17} /><span><strong>{t('discovery.state_unsupported')}</strong>{run.error_message && <small>{run.error_code}: {run.error_message}</small>}</span></div>
      )}
      {run.status === 'failed' && (
        <div className={styles.errorState} role="alert"><IconTriangleAlert size={17} /><div><strong>{t('discovery.state_failed')}</strong><span>{run.error_message ?? t('discovery.state_failed_desc')}</span>{run.error_code && <code>{run.error_code}</code>}</div></div>
      )}
      {run.status === 'succeeded' && run.discovered_model_count === 0 && (
        <EmptyTable title={t('discovery.state_empty')} description={t('discovery.state_empty_desc')} />
      )}
      <div className={styles.diffGrid}>
        <DiffColumn title={t('discovery.diff_column.added')} tone="success" entries={diff.added} />
        <DiffColumn title={t('discovery.diff_column.changed')} tone="warning" entries={diff.changed} />
        <DiffColumn title={t('discovery.diff_column.missing')} tone="danger" entries={diff.missing} />
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
  const { t } = useTranslation('console');
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
      setValidationError(t('discovery.no_field_changes'));
      return;
    }
    const metadata = metadataFromDraft(draft, dirty);
    const invalidNumber = (['context_window', 'max_input_tokens', 'max_output_tokens'] as const)
      .some((field) => metadata[field] !== undefined && metadata[field] !== null
        && (!Number.isFinite(metadata[field] as number) || (metadata[field] as number) <= 0));
    if (invalidNumber) {
      setValidationError(t('discovery.validate_tokens'));
      return;
    }
    setValidationError('');
    onSubmit(metadata);
  };

  return (
    <form id="source-model-editor-form" className={styles.page} onSubmit={submit}>
      <div className={styles.modelIdentity}>
        <code>{model.upstream_model_id}</code>
        <span><StatusPill tone={statusTone(model.confirmation_status)}>{t(`discovery.confirm_state.${model.confirmation_status}`)}</StatusPill><StatusPill tone={statusTone(model.availability_status)}>{t(`discovery.availability_state.${model.availability_status}`)}</StatusPill></span>
      </div>
      <ModelMetadataFields draft={draft} fieldSources={model.field_sources} disabled={busy} onChange={change} />
      <FormError message={validationError || error} />
    </form>
  );
}

export function ModelDiscoveryPage({ api, refreshRevision = 0, onBusyChange }: ModelDiscoveryPageProps) {
  const { t } = useTranslation('console');
  const localize = useLocalizedApiError();
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

  const knownFieldSources = ['user', 'preset', 'upstream', 'unknown'] as const;
  const fieldSourceLabel = (value: string): string => (
    (knownFieldSources as readonly string[]).includes(value) ? t(`discovery.field_source.${value}`) : value
  );

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
        ? t('discovery.message_unsupported', { code: result.run.error_code ?? 'discovery_unsupported' })
        : result.run.status === 'failed'
          ? t('discovery.state_failed_recorded', { id: result.run.id })
          : t('discovery.state_ok', { id: result.run.id, count: result.run.discovered_model_count }),
    );
  };

  const saveModel = (metadata: ModelMetadataValues) => {
    if (!editingModel) return;
    void runMutation(
      () => api.editSourceModel(editingModel.source_id, {
        upstream_model_id: editingModel.upstream_model_id,
        metadata,
      }),
      t('discovery.message_saved', { name: editingModel.upstream_model_id }),
    );
  };

  const confirmSelected = () => {
    const models = eligibleModels
      .filter((model) => selectedModels.has(model.upstream_model_id))
      .map((model) => ({ upstream_model_id: model.upstream_model_id, metadata: {} }));
    if (!effectiveSourceId || models.length === 0) return;
    void runMutation(
      () => api.confirmSourceModels(effectiveSourceId, models),
      t('discovery.message_confirmed', { count: models.length }),
    );
  };

  const toggleAll = (checked: boolean) => {
    setSelectedModels(checked ? new Set(eligibleModels.map((model) => model.upstream_model_id)) : new Set());
  };

  if (contextQuery.loading && !context) return <LoadingState label={t('discovery.loading')} />;
  if (contextQuery.error && !context) return <ErrorState error={contextQuery.error} onRetry={contextQuery.reload} />;
  if (!context) return null;
  if (context.sources.length === 0) return <EmptyTable title={t('discovery.no_sources_title')} description={t('discovery.no_sources_desc')} />;

  return (
    <section className={styles.page} data-od-id="page-model-discovery">
      <PageActions>
        <div className={styles.inlineActions}>
          <SelectField label={t('discovery.source')} value={effectiveSourceId} onChange={(event) => changeSource(event.target.value)}>
            {context.sources.map((item) => <option key={item.id} value={item.id}>{item.display_name} · {item.id}</option>)}
          </SelectField>
          <SelectField label={t('discovery.account')} value={effectiveAccountId} disabled={enabledAccounts.length === 0} onChange={(event) => setAccountId(event.target.value)}>
            {enabledAccounts.length === 0 && <option value="">{t('discovery.no_enabled_account')}</option>}
            {enabledAccounts.map((account) => <option key={account.id} value={account.id}>{account.display_name} · {account.id}</option>)}
          </SelectField>
        </div>
        <div className={styles.rowActions}>
          <Button variant="secondary" onClick={() => { contextQuery.reload(); discoveryQuery.reload(); }} loading={contextQuery.refreshing || discoveryQuery.refreshing}><IconRefreshCw size={14} />{t('common.refresh')}</Button>
          <Button variant="primary" onClick={runDiscovery} loading={mutationBusy} disabled={!effectiveAccountId}>
            <IconPlay size={14} />{t('discovery.run_button')}
          </Button>
        </div>
      </PageActions>

      <SuccessNotice message={notice} onDismiss={() => setNotice('')} />
      {contextQuery.error && <ErrorState error={contextQuery.error} onRetry={contextQuery.reload} />}
      {mutationError && !editingModel && !confirmOpen && <ErrorState error={mutationError} />}

      {discoveryDefinition?.support === 'unsupported' && !discovery?.latest && (
        <div className={styles.warningState} role="status">
          <IconTriangleAlert size={17} />
          <span><strong>{t('discovery.declares_unsupported')}</strong><small>{discoveryDefinition.reason}</small></span>
        </div>
      )}

      <Card title={t('discovery.latest_run_card')} subtitle={t('discovery.latest_run_subtitle')} extra={<StatusPill tone="accent">{source?.provider_preset_id}@{source?.provider_preset_version}</StatusPill>}>
        {discoveryQuery.loading && !discovery ? <LoadingState label={t('discovery.loading_run')} /> : discoveryQuery.error ? <ErrorState error={discoveryQuery.error} onRetry={discoveryQuery.reload} /> : <LatestRunPanel latest={discovery?.latest?.run.source_id === effectiveSourceId ? discovery.latest : null} />}
      </Card>

      <FilterBar>
        <label>{t('discovery.confirmation_filter')}<select value={confirmationFilter} onChange={(event) => { setConfirmationFilter(event.target.value as CatalogStatus | ''); setSelectedModels(new Set()); }}><option value="">{t('common.all')}</option><option value="pending">{t('discovery.confirm_state.pending')}</option><option value="confirmed">{t('discovery.confirm_state.confirmed')}</option><option value="unavailable">{t('discovery.confirm_state.unavailable')}</option></select></label>
        <label>{t('discovery.availability_filter')}<select value={availabilityFilter} onChange={(event) => { setAvailabilityFilter(event.target.value as CatalogAvailability | ''); setSelectedModels(new Set()); }}><option value="">{t('common.all')}</option><option value="unknown">{t('discovery.availability_state.unknown')}</option><option value="available">{t('discovery.availability_state.available')}</option><option value="unavailable">{t('discovery.availability_state.unavailable')}</option></select></label>
        <span className={styles.filterMeta}>{t('discovery.source_model_count', { count: visibleModels.length })}</span>
      </FilterBar>

      {discoveryQuery.loading && !discovery ? <LoadingState label={t('discovery.loading_models')} /> : visibleModels.length === 0 ? (
        <EmptyTable
          title={discovery?.latest?.run.status === 'unsupported' ? t('discovery.empty_no_auto') : t('discovery.empty_no_match')}
          description={discovery?.latest?.run.status === 'failed' ? t('discovery.empty_no_match_desc') : t('discovery.empty_no_auto_desc')}
        />
      ) : (
        <Card variant="flush" title={t('discovery.models_card')} subtitle={t('discovery.models_subtitle')} extra={(
          <div className={styles.rowActions}>
            <StatusPill tone="warning">{t('discovery.selected_count', { count: selectedModels.size })}</StatusPill>
            <Button size="sm" variant="secondary" onClick={() => setConfirmOpen(true)} disabled={selectedModels.size === 0 || mutationBusy}><IconCircleCheck size={14} />{t('discovery.confirm_selected')}</Button>
          </div>
        )}>
          <TableScroll label={t('discovery.table_aria')}>
            <table className={styles.table}>
              <thead><tr><th><label className={styles.tableCheckbox}><input aria-label={t('discovery.select_all_aria')} type="checkbox" checked={eligibleModels.length > 0 && eligibleModels.every((model) => selectedModels.has(model.upstream_model_id))} onChange={(event) => toggleAll(event.target.checked)} /><span aria-hidden="true" /></label></th><th>{t('discovery.column.upstream_model')}</th><th>{t('discovery.column.confirmation')}</th><th>{t('discovery.column.availability')}</th><th>{t('discovery.column.metadata')}</th><th>{t('discovery.column.field_source')}</th><th>{t('discovery.column.preset_match')}</th><th>{t('discovery.column.last_discovered')}</th><th>{t('common.actions')}</th></tr></thead>
              <tbody>{visibleModels.map((model) => {
                const eligible = model.confirmation_status === 'pending' && model.availability_status === 'available';
                return (
                  <tr key={`${model.source_id}:${model.upstream_model_id}`}>
                    <td><label className={styles.tableCheckbox}><input aria-label={t('discovery.select_row_aria', { model: model.upstream_model_id })} type="checkbox" disabled={!eligible} checked={selectedModels.has(model.upstream_model_id)} onChange={(event) => setSelectedModels((current) => { const next = new Set(current); if (event.target.checked) next.add(model.upstream_model_id); else next.delete(model.upstream_model_id); return next; })} /><span aria-hidden="true" /></label></td>
                    <td><code>{model.upstream_model_id}</code></td>
                    <td><StatusPill tone={statusTone(model.confirmation_status)}>{t(`discovery.confirm_state.${model.confirmation_status}`)}</StatusPill></td>
                    <td><StatusPill tone={statusTone(model.availability_status)}>{t(`discovery.availability_state.${model.availability_status}`)}</StatusPill></td>
                    <td><span className={styles.primaryText}><strong>{metadataSummary(model) || t('discovery.unnamed_metadata')}</strong><small>context {String(model.metadata.context_window ?? 'unknown')}</small></span></td>
                    <td><span className={styles.secondaryText}>{metadataSourcesSummary(model, fieldSourceLabel)}</span></td>
                    <td>{model.matched_model_preset_id ? <code>{model.matched_model_preset_id}@{model.matched_model_preset_version}</code> : <StatusPill>{t('discovery.none')}</StatusPill>}</td>
                    <td>{formatDateTime(model.last_discovered_at)}</td>
                    <td><Button size="sm" variant="ghost" onClick={() => setEditingModel(model)} disabled={model.confirmation_status !== 'pending'}><IconPencil size={14} />{t('common.edit')}</Button></td>
                  </tr>
                );
              })}</tbody>
            </table>
          </TableScroll>
        </Card>
      )}

      <Modal
        open={Boolean(editingModel)}
        title={t('discovery.edit_pending_title')}
        width={760}
        onClose={() => !mutationBusy && setEditingModel(undefined)}
        closeDisabled={mutationBusy}
        footer={(
          <>
            <Button variant="secondary" onClick={() => setEditingModel(undefined)} disabled={mutationBusy}>{t('common.cancel')}</Button>
            <Button type="submit" form="source-model-editor-form" loading={mutationBusy}><IconPencil size={14} />{t('discovery.save_user_fields')}</Button>
          </>
        )}
      >
        {editingModel && <SourceModelEditor key={`${editingModel.source_id}:${editingModel.upstream_model_id}`} model={editingModel} busy={mutationBusy} error={mutationError ? localize(mutationError) : undefined} onSubmit={saveModel} />}
      </Modal>

      <ConfirmDialog
        open={confirmOpen}
        title={t('discovery.batch_confirm_title')}
        description={t('discovery.batch_confirm_desc', { count: selectedModels.size })}
        confirmLabel={t('discovery.confirm_models')}
        busy={mutationBusy}
        onCancel={() => !mutationBusy && setConfirmOpen(false)}
        onConfirm={confirmSelected}
      />
    </section>
  );
}
