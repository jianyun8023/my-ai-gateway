import type {
  AdminErrorShape,
  CatalogAvailability,
  CatalogStatus,
  GatewayAdminResources,
  ModelMetadataValues,
  SourceModel
} from '@/admin-api';
import { normalizeAdminError } from '@/admin-api';
import { Button } from '@/components/ui/Button';
import { Card } from '@/components/ui/Card';
import { SelectField } from '@/components/ui/FormField';
import {
  IconCircleCheck,
  IconPencil,
  IconPlay,
  IconRefreshCw,
  IconSlidersHorizontal,
  IconTriangleAlert,
} from '@/components/ui/icons';
import { LoadingState } from '@/components/ui/LoadingState';
import { Modal } from '@/components/ui/Modal';
import { StatusPill } from '@/components/ui/StatusPill';
import { TableScroll } from '@/components/ui/TableScroll';
import styles from '@/features/control-plane/ControlPlane.module.scss';
import { LatestRunPanel } from '@/features/control-plane/discovery/LatestRunPanel';
import { metadataSourcesSummary, metadataSummary, sourceDiscoveryDefinition, statusTone, type DiscoveryContext, type DiscoveryView } from '@/features/control-plane/discovery/model';
import { SourceModelCapabilitiesEditor } from '@/features/control-plane/discovery/SourceModelCapabilitiesEditor';
import { SourceModelEditor } from '@/features/control-plane/discovery/SourceModelEditor';
import { ConfirmDialog, EmptyTable, ErrorState, FilterBar, PageActions, SuccessNotice } from '@/features/control-plane/shared';
import { useAdminQuery } from '@/hooks/useAdminQuery';
import { useLocalizedApiError } from '@/hooks/useLocalizedApiError';
import { PROTOCOL_LABELS } from '@/lib/protocols';
import { formatDateTime } from '@/utils/format';
import { useCallback, useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';

interface ModelDiscoveryPageProps {
  api: GatewayAdminResources;
  refreshRevision?: number;
  onBusyChange?: (busy: boolean) => void;
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
  const [capabilityModel, setCapabilityModel] = useState<SourceModel>();
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

      <Card title={t('discovery.latest_run_card')} extra={<StatusPill tone="accent">{source?.provider_preset_id}@{source?.provider_preset_version}</StatusPill>}>
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
        <Card variant="flush" title={t('discovery.models_card')} extra={(
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
                    <td>
                      <div className={styles.rowActions}>
                        <Button size="sm" variant="ghost" onClick={() => setCapabilityModel(model)}><IconSlidersHorizontal size={14} />{t('discovery.capabilities_action')}</Button>
                        <Button size="sm" variant="ghost" onClick={() => setEditingModel(model)} disabled={model.confirmation_status !== 'pending'}><IconPencil size={14} />{t('common.edit')}</Button>
                      </div>
                    </td>
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

      <Modal
        open={Boolean(capabilityModel)}
        title={t('discovery.capabilities_title', { model: capabilityModel?.upstream_model_id ?? '' })}
        width={820}
        onClose={() => setCapabilityModel(undefined)}
      >
        <p className={styles.secondaryText}>{t('discovery.capabilities_subtitle')}</p>
        {capabilityModel && (
          <SourceModelCapabilitiesEditor
            key={`${capabilityModel.source_id}:${capabilityModel.upstream_model_id}`}
            api={api}
            model={capabilityModel}
            onSaved={(protocol) => {
              setNotice(t('discovery.capability_saved', { protocol: PROTOCOL_LABELS[protocol] }));
              discoveryQuery.reload();
            }}
          />
        )}
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
