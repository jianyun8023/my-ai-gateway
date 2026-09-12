import { FormActions } from '@/components/ui/FormActions';
import { clearOperationNotification, notifySuccess } from '@/components/ui/notifications';
import { Notice } from '@/components/ui/Notice';
import { Checkbox, Table } from '@mantine/core';
import { useOverlayState } from '@/components/ui/useOverlayState';
import type {
  AdminErrorShape,
  CatalogAvailability,
  CatalogStatus,
  GatewayAdminResources,
  ModelMetadataValues,
  SourceModel,
} from '@/admin-api';
import { normalizeAdminError } from '@/admin-api';
import { Button } from '@/components/ui/Button';
import { Card } from '@/components/ui/Card';
import { SelectField, TextField } from '@/components/ui/FormField';
import {
  IconCircleCheck,
  IconPencil,
  IconPlay,
  IconRefreshCw,
  IconSlidersHorizontal,
} from '@/components/ui/icons';
import { LoadingState } from '@/components/ui/LoadingState';
import { MetricCard } from '@/components/ui/MetricCard';
import { Modal } from '@/components/ui/Modal';
import { SegmentedTabs } from '@/components/ui/SegmentedTabs';
import { StatusPill } from '@/components/ui/StatusPill';
import { TableScroll } from '@/components/ui/TableScroll';
import styles from '@/features/control-plane/ControlPlane.module.scss';
import { SourceModelCapabilitiesEditor } from '@/features/control-plane/discovery/SourceModelCapabilitiesEditor';
import { SourceModelEditor } from '@/features/control-plane/discovery/SourceModelEditor';
import {
  buildSyncStats,
  capabilitySummary,
  diffChangeMap,
  statusTone,
  type DiscoveryChangeKind,
} from '@/features/control-plane/discovery/model';
import { ConfirmDialog, EmptyTable, ErrorState, FilterBar, PageActions } from '@/features/control-plane/shared';
import { useAdminQuery } from '@/hooks/useAdminQuery';
import { useLocalizedApiError } from '@/hooks/useLocalizedApiError';
import { PROTOCOL_LABELS } from '@/lib/protocols';
import { sourceRouteHash, type SourceSection } from '@/lib/consoleNavigation';
import { formatDateTime } from '@/utils/format';
import { useCallback, useState } from 'react';
import { useTranslation } from 'react-i18next';

interface ModelReviewPageProps {
  api: GatewayAdminResources;
  refreshRevision?: number;
  onBusyChange?: (busy: boolean) => void;
  sourceId: string;
  onOpenSource?: (sourceId: string, section?: SourceSection) => void;
  onNavigatePage?: (page: 'models') => void;
}

type ConfirmationFilter = CatalogStatus | '';

export function ModelReviewPage({ api, refreshRevision = 0, onBusyChange, sourceId, onOpenSource, onNavigatePage }: ModelReviewPageProps) {
  const { t } = useTranslation('console');
  const localize = useLocalizedApiError();
  const [confirmationFilter, setConfirmationFilter] = useState<ConfirmationFilter>('');
  const [availabilityFilter, setAvailabilityFilter] = useState<CatalogAvailability | ''>('');
  const [search, setSearch] = useState('');
  const [selectedModels, setSelectedModels] = useState<Set<string>>(() => new Set());
  const { value: editingModel, setValue: setEditingModel, opened: editingModelOpen, afterExit: editingModelAfterExit } = useOverlayState<SourceModel>();
  const { value: capabilityModel, setValue: setCapabilityModel, opened: capabilityModelOpen, afterExit: capabilityModelAfterExit } = useOverlayState<SourceModel>();
  const [confirmOpen, setConfirmOpen] = useState(false);
  const [mutationBusy, setMutationBusy] = useState(false);
  const [mutationError, setMutationError] = useState<AdminErrorShape>();

  const openSource = (targetId: string, section?: SourceSection) => {
    if (onOpenSource) {
      onOpenSource(targetId, section);
      return;
    }
    window.location.hash = sourceRouteHash(targetId, section);
  };

  const loadContext = useCallback(async (signal: AbortSignal) => {
    const [sources, accounts] = await Promise.all([api.sources(signal), api.accounts(signal)]);
    return {
      source: sources.find((item) => item.id === sourceId),
      enabledAccounts: accounts.filter((account) => account.source_id === sourceId && account.enabled),
    };
  }, [api, sourceId]);
  const contextQuery = useAdminQuery({ load: loadContext, refreshRevision, onBusyChange });
  const context = contextQuery.data;
  const enabledAccounts = context?.enabledAccounts ?? [];

  const loadReview = useCallback(async (signal: AbortSignal) => {
    // 拉取全量模型后在前端筛选，保证页签计数基于完整目录而不是当前筛选结果。
    const [latest, models] = await Promise.all([
      api.latestDiscovery(sourceId, signal),
      api.sourceModels(sourceId, {}, signal),
    ]);
    return { stats: buildSyncStats(latest, models), changes: diffChangeMap(latest) };
  }, [api, sourceId]);
  const reviewQuery = useAdminQuery({ load: loadReview, queryKey: sourceId, refreshRevision });
  const review = reviewQuery.data;
  const stats = review?.stats;
  const allModels = stats?.models ?? [];
  const pendingCount = allModels.filter((model) => model.confirmation_status === 'pending').length;
  const confirmedCount = allModels.filter((model) => model.confirmation_status === 'confirmed').length;
  const unavailableCount = allModels.filter((model) => model.confirmation_status === 'unavailable').length;
  const visibleModels = allModels.filter((model) => (
    (!confirmationFilter || model.confirmation_status === confirmationFilter)
    && (!availabilityFilter || model.availability_status === availabilityFilter)
  ));
  const filteredModels = visibleModels.filter((model) => {
    const term = search.trim().toLowerCase();
    if (!term) return true;
    return model.upstream_model_id.toLowerCase().includes(term)
      || (typeof model.metadata.display_name === 'string' && model.metadata.display_name.toLowerCase().includes(term));
  });
  // 全选、选中数量与批量确认都以当前可见（含搜索）范围为准，避免确认被搜索隐藏的模型。
  const eligibleModels = filteredModels.filter((model) => model.confirmation_status === 'pending' && model.availability_status === 'available');
  const confirmableSelection = eligibleModels.filter((model) => selectedModels.has(model.upstream_model_id));
  const effectiveAccountId = enabledAccounts[0]?.id ?? '';

  const runMutation = async (
    operation: () => Promise<unknown>,
    successMessage?: string,
  ) => {
    if (mutationBusy) return;
    clearOperationNotification();
    setMutationBusy(true);
    setMutationError(undefined);
    onBusyChange?.(true);
    try {
      await operation();
      if (successMessage) notifySuccess(successMessage);
      setEditingModel(undefined);
      setConfirmOpen(false);
      setSelectedModels(new Set());
      reviewQuery.reload();
    } catch (error) {
      setMutationError(normalizeAdminError(error));
    } finally {
      setMutationBusy(false);
      onBusyChange?.(false);
    }
  };

  const runDiscovery = () => {
    if (!effectiveAccountId) return;
    void runMutation(
      () => api.runDiscovery(sourceId, effectiveAccountId),
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

  const confirmModels = (models: SourceModel[]) => {
    if (models.length === 0) return;
    void runMutation(
      () => api.confirmSourceModels(sourceId, models.map((model) => ({ upstream_model_id: model.upstream_model_id, metadata: {} }))),
      t('discovery.message_confirmed', { count: models.length }),
    );
  };

  const confirmSelected = () => {
    confirmModels(confirmableSelection);
  };

  const toggleAll = (checked: boolean) => {
    setSelectedModels(checked ? new Set(eligibleModels.map((model) => model.upstream_model_id)) : new Set());
  };

  const gotoModels = () => {
    if (onNavigatePage) {
      onNavigatePage('models');
      return;
    }
    window.location.hash = '#models';
  };

  const changeLabel = (kind: DiscoveryChangeKind, changedFields: string[]): string => {
    const known = changedFields.filter((field) => field.startsWith('metadata.'));
    if (kind === 'added') return t('discovery.change_kind.added');
    if (kind === 'missing') return t('discovery.change_kind.missing');
    return known.length > 0
      ? t('discovery.change_kind.changed_with_fields', { fields: known.map((field) => field.replace('metadata.', '')).join(', ') })
      : t('discovery.change_kind.changed');
  };

  if (contextQuery.loading && !context) return <LoadingState label={t('discovery.loading')} />;
  if (contextQuery.error && !context) return <ErrorState error={contextQuery.error} onRetry={contextQuery.reload} />;
  if (!context) return null;
  if (!context.source) {
    return (
      <section className={styles.page} data-od-id="page-model-review">
        <EmptyTable title={t('sources.detail.missing_title', { id: sourceId })} description={t('sources.detail.missing_desc')} />
        {contextQuery.error && <ErrorState error={contextQuery.error} onRetry={contextQuery.reload} />}
        <div className={styles.cardActions}>
          <Button variant="secondary" onClick={() => openSource('')}>{t('sources.detail.back_to_list')}</Button>
        </div>
      </section>
    );
  }

  const source = context.source;
  const latest = stats?.latest ?? null;

  return (
    <section className={styles.page} data-od-id="page-model-review">
      <div className={styles.breadcrumbBar}>
        <Button size="sm" variant="ghost" onClick={() => openSource('')}>{t('sources.detail.back_to_list')}</Button>
        <span aria-hidden="true">/</span>
        <Button size="sm" variant="ghost" onClick={() => openSource(sourceId)}>{source.display_name}</Button>
        <span aria-hidden="true">/</span>
        <strong>{t('sources.review.title')}</strong>
      </div>

      <p className={styles.secondaryText}>{t('sources.review.subtitle')}</p>

      <PageActions>
        <span />
        <div className={styles.rowActions}>
          <Button variant="secondary" onClick={reviewQuery.reload} loading={reviewQuery.refreshing}><IconRefreshCw size={14} />{t('common.refresh')}</Button>
          <Button variant="secondary" onClick={runDiscovery} loading={mutationBusy} disabled={!effectiveAccountId}>
            <IconPlay size={14} />{t('sources.review.rerun')}
          </Button>
          <Button variant="secondary" onClick={() => setConfirmOpen(true)} disabled={confirmableSelection.length === 0 || mutationBusy}>
            <IconCircleCheck size={14} />{t('discovery.confirm_selected')}{confirmableSelection.length > 0 ? ` (${confirmableSelection.length})` : ''}
          </Button>
          <Button variant="primary" onClick={gotoModels}>{t('sources.review.goto_models')}</Button>
        </div>
      </PageActions>

      {contextQuery.error && <ErrorState error={contextQuery.error} onRetry={contextQuery.reload} />}
      {reviewQuery.error && <ErrorState error={reviewQuery.error} onRetry={reviewQuery.reload} />}
      {mutationError && !editingModel && !confirmOpen && <ErrorState error={mutationError} />}

      <Card>
        {reviewQuery.loading && !review ? <LoadingState label={t('discovery.loading_run')} /> : reviewQuery.error && !review ? <ErrorState error={reviewQuery.error} onRetry={reviewQuery.reload} /> : !latest ? (
          <EmptyTable title={t('sources.review.no_run_title')} description={t('sources.review.no_run_desc')} />
        ) : (
          <div className={styles.metaStrip}>
            <span>{t('sources.review.run_source')}<strong>{source.display_name}</strong></span>
            <span>{t('sources.review.run_state')}<StatusPill tone={statusTone(latest.run.status)}>{t(`discovery.run_state.${latest.run.status}`, { defaultValue: latest.run.status })}</StatusPill></span>
            <span>{t('sources.review.run_account')}<strong className={styles.mono}>{latest.run.account_id ?? t('discovery.none')}</strong></span>
            <span>{t('sources.review.run_time')}<strong>{stats?.lastSyncAt ? formatDateTime(stats.lastSyncAt) : '—'}</strong></span>
            <span>{t('sources.review.run_models')}<strong>{latest.run.discovered_model_count}</strong></span>
          </div>
        )}
      </Card>

      {stats && (
        <div className={styles.statsGrid} data-count="5">
          <MetricCard label={t('discovery.diff_column.added')} value={stats.diffAvailable ? String(stats.addedCount) : '—'} tone={stats.diffAvailable && stats.addedCount > 0 ? 'success' : undefined} />
          <MetricCard label={t('discovery.diff_column.changed')} value={stats.diffAvailable ? String(stats.changedCount) : '—'} tone={stats.diffAvailable && stats.changedCount > 0 ? 'warning' : undefined} />
          <MetricCard label={t('discovery.diff_column.missing')} value={stats.diffAvailable ? String(stats.missingCount) : '—'} tone={stats.diffAvailable && stats.missingCount > 0 ? 'warning' : undefined} />
          <MetricCard label={t('sources.review.stat_unchanged')} value={stats.diffAvailable ? String(stats.unchangedCount) : '—'} />
          <MetricCard label={t('sources.review.stat_pending')} value={String(stats.pendingCount)} tone={stats.pendingCount > 0 ? 'warning' : 'success'} />
        </div>
      )}

      {latest?.run.status === 'failed' && (
        <Notice>
          <strong>{t('discovery.state_failed')}</strong>
          <span>{latest.run.error_message ?? t('discovery.state_failed_desc')}</span>
          {latest.run.error_code && <code>{latest.run.error_code}</code>}
        </Notice>
      )}
      {latest?.run.status === 'unsupported' && (
        <Notice tone="warning">
          <strong>{t('discovery.state_unsupported')}</strong>
          {latest.run.error_message && <small>{latest.run.error_message}</small>}
          {latest.run.error_code && <code>{latest.run.error_code}</code>}
        </Notice>
      )}
      {latest?.run.status === 'succeeded' && latest.run.discovered_model_count === 0 && (
        <Notice tone="warning">
          <strong>{t('discovery.state_empty')}</strong>
          <small>{t('discovery.state_empty_desc')}</small>
        </Notice>
      )}

      <FilterBar label={t('discovery.filters_aria')}>
        <SegmentedTabs
          id="review-confirmation-tabs"
          mode="group"
          value={confirmationFilter}
          label={t('discovery.confirmation_filter')}
          options={[
            { value: '', label: t('common.all'), count: allModels.length },
            { value: 'pending', label: t('discovery.confirm_state.pending'), count: pendingCount },
            { value: 'confirmed', label: t('discovery.confirm_state.confirmed'), count: confirmedCount },
            { value: 'unavailable', label: t('discovery.confirm_state.unavailable'), count: unavailableCount },
          ]}
          onChange={(value) => { setConfirmationFilter(value as ConfirmationFilter); setSelectedModels(new Set()); }}
        />
        <SelectField
          label={t('discovery.availability_filter')}
          value={availabilityFilter}
          data={[
            { value: '', label: t('common.all') },
            { value: 'unknown', label: t('discovery.availability_state.unknown') },
            { value: 'available', label: t('discovery.availability_state.available') },
            { value: 'unavailable', label: t('discovery.availability_state.unavailable') },
          ]}
          onChange={(value) => { setAvailabilityFilter(value as CatalogAvailability | ''); setSelectedModels(new Set()); }}
        />
        <TextField
          label={t('sources.review.search')}
          value={search}
          onChange={(event) => setSearch(event.target.value)}
          autoComplete="off"
        />
        <span className={styles.filterMeta}>{t('discovery.source_model_count', { count: filteredModels.length })}</span>
      </FilterBar>

      {reviewQuery.loading && !review ? <LoadingState label={t('discovery.loading_models')} /> : reviewQuery.error && !review ? null : filteredModels.length === 0 ? (
        <EmptyTable
          title={latest?.run.status === 'unsupported' ? t('discovery.empty_no_auto') : search ? t('sources.review.search_empty') : t('discovery.empty_no_match')}
          description={latest?.run.status === 'failed' ? t('discovery.empty_no_match_desc') : search ? undefined : t('discovery.empty_no_auto_desc')}
        />
      ) : (
        <Card variant="flush" title={t('discovery.models_card')}>
          <TableScroll label={t('discovery.table_aria')}>
            <Table className={styles.table}>
              <Table.Thead><Table.Tr>
                <Table.Th scope="col"><Checkbox className={styles.tableCheckbox} label=" " classNames={{ label: styles.tableCheckboxLabel }} aria-label={t('discovery.select_all_aria')} checked={eligibleModels.length > 0 && eligibleModels.every((model) => selectedModels.has(model.upstream_model_id))} onChange={(event) => toggleAll(event.target.checked)} /></Table.Th>
                <Table.Th scope="col">{t('discovery.column.upstream_model')}</Table.Th>
                <Table.Th scope="col">{t('sources.review.column.suggested')}</Table.Th>
                <Table.Th scope="col">{t('sources.review.column.capabilities')}</Table.Th>
                <Table.Th scope="col">{t('sources.review.column.change')}</Table.Th>
                <Table.Th scope="col">{t('discovery.column.confirmation')}</Table.Th>
                <Table.Th scope="col">{t('common.actions')}</Table.Th>
              </Table.Tr></Table.Thead>
              <Table.Tbody>{filteredModels.map((model) => {
                const eligible = model.confirmation_status === 'pending' && model.availability_status === 'available';
                const change = review?.changes.get(model.upstream_model_id);
                return (
                  <Table.Tr key={`${model.source_id}:${model.upstream_model_id}`}>
                    <Table.Td onClick={(event) => event.stopPropagation()}><Checkbox className={styles.tableCheckbox} label=" " classNames={{ label: styles.tableCheckboxLabel }} aria-label={t('discovery.select_row_aria', { model: model.upstream_model_id })} disabled={!eligible} checked={selectedModels.has(model.upstream_model_id)} onChange={(event) => setSelectedModels((current) => { const next = new Set(current); if (event.target.checked) next.add(model.upstream_model_id); else next.delete(model.upstream_model_id); return next; })} /></Table.Td>
                    <Table.Td><span className={styles.primaryText}><code>{model.upstream_model_id}</code><small className={styles.secondaryText}>{t(`discovery.availability_state.${model.availability_status}`)}</small></span></Table.Td>
                    <Table.Td>{typeof model.metadata.logical_model_name === 'string' && model.metadata.logical_model_name ? model.metadata.logical_model_name : <span className={styles.secondaryText}>—</span>}</Table.Td>
                    <Table.Td><span className={styles.secondaryText}>{capabilitySummary(model, t) || '—'}</span></Table.Td>
                    <Table.Td>{!review?.stats.diffAvailable ? (
                      <span className={styles.secondaryText}>{t('discovery.change_kind.unknown')}</span>
                    ) : change ? (
                      <span className={styles.primaryText}>
                        <StatusPill tone={change.kind === 'added' ? 'success' : change.kind === 'missing' ? 'danger' : 'warning'}>{changeLabel(change.kind, change.changedFields)}</StatusPill>
                        {change.kind === 'changed' && change.changedFields.length > 0 && <small className={styles.secondaryText}>{change.changedFields.join(', ')}</small>}
                      </span>
                    ) : <span className={styles.secondaryText}>{t('discovery.diff_none')}</span>}</Table.Td>
                    <Table.Td><StatusPill tone={statusTone(model.confirmation_status)}>{t(`discovery.confirm_state.${model.confirmation_status}`)}</StatusPill></Table.Td>
                    <Table.Td onClick={(event) => event.stopPropagation()}>
                      <div className={styles.rowActions}>
                        {eligible && (
                          <Button size="sm" variant="ghost" onClick={() => confirmModels([model])}><IconCircleCheck size={14} />{t('sources.review.confirm_action')}</Button>
                        )}
                        <Button size="sm" variant="ghost" onClick={() => setCapabilityModel(model)}><IconSlidersHorizontal size={14} />{t('discovery.capabilities_action')}</Button>
                        <Button size="sm" variant="ghost" onClick={() => setEditingModel(model)} disabled={model.confirmation_status !== 'pending'}><IconPencil size={14} />{t('common.edit')}</Button>
                      </div>
                    </Table.Td>
                  </Table.Tr>
                );
              })}</Table.Tbody>
            </Table>
          </TableScroll>
        </Card>
      )}

      <Card className={styles.flowCard}>
        <strong>{t('sources.review.explain_title')}</strong>
        <span className={styles.secondaryText}>{t('sources.review.explain_body')}</span>
        <span className={styles.secondaryText}>{t('sources.review.explain_flow')}</span>
      </Card>

      <Modal
        open={editingModelOpen}
        onExitTransitionEnd={editingModelAfterExit}
        title={t('discovery.edit_pending_title')}
        width={760}
        onClose={() => !mutationBusy && setEditingModel(undefined)}
        closeDisabled={mutationBusy}
        footer={(
          <FormActions form="source-model-editor-form" cancelLabel={t('common.cancel')} submitLabel={t('discovery.save_user_fields')}
            submitIcon={<IconPencil size={14} />} busy={mutationBusy} onCancel={() => setEditingModel(undefined)} />
        )}
      >
        {editingModel && <SourceModelEditor key={`${editingModel.source_id}:${editingModel.upstream_model_id}`} model={editingModel} busy={mutationBusy} error={mutationError ? localize(mutationError) : undefined} onSubmit={saveModel} />}
      </Modal>

      <Modal
        open={capabilityModelOpen}
        onExitTransitionEnd={capabilityModelAfterExit}
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
              notifySuccess(t('discovery.capability_saved', { protocol: PROTOCOL_LABELS[protocol] }));
              reviewQuery.reload();
            }}
          />
        )}
      </Modal>

      <ConfirmDialog
        open={confirmOpen}
        title={t('discovery.batch_confirm_title')}
        description={<>{t('discovery.batch_confirm_desc', { count: confirmableSelection.length })}{mutationError && <ErrorState error={mutationError} />}</>}
        confirmLabel={t('discovery.confirm_models')}
        busy={mutationBusy}
        onCancel={() => !mutationBusy && setConfirmOpen(false)}
        onConfirm={confirmSelected}
      />
    </section>
  );
}
