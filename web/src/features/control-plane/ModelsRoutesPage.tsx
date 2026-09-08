import { clearOperationNotification, notifySuccess } from '@/components/ui/notifications';
import { Table } from '@mantine/core';
import { useOverlayState } from '@/components/ui/useOverlayState';
import type {
  AdminErrorShape,
  GatewayAdminResources,
  LogicalModelWriteInput,
  ModelBindingWriteInput,
  RouteWriteInput
} from '@/admin-api';
import { normalizeAdminError } from '@/admin-api';
import { Button } from '@/components/ui/Button';
import { Card } from '@/components/ui/Card';
import { IconButton } from '@/components/ui/IconButton';
import {
  IconEye,
  IconPencil,
  IconPlus,
  IconPower,
  IconRefreshCw,
  IconTrash2,
} from '@/components/ui/icons';
import { LoadingState } from '@/components/ui/LoadingState';
import { Modal } from '@/components/ui/Modal';
import { SegmentedTabs } from '@/components/ui/SegmentedTabs';
import { StatusPill } from '@/components/ui/StatusPill';
import { TableScroll } from '@/components/ui/TableScroll';
import styles from '@/features/control-plane/ControlPlane.module.scss';
import { BindingForm } from '@/features/control-plane/models/BindingForm';
import { resolvedCellsForBinding, statusTone, type CatalogData, type DeleteTarget, type DetailTarget, type Editor } from '@/features/control-plane/models/catalog';
import { EntityDetailDrawer } from '@/features/control-plane/models/EntityDetailDrawer';
import { LogicalModelForm } from '@/features/control-plane/models/LogicalModelForm';
import { RouteForm } from '@/features/control-plane/models/RouteForm';
import { RuntimeBindingSummary } from '@/features/control-plane/models/RuntimeBindingSummary';
import { ConfirmDialog, EmptyTable, ErrorState, FormError, PageActions, ProtocolPill, Toggle } from '@/features/control-plane/shared';
import { useAdminQuery } from '@/hooks/useAdminQuery';
import { useLocalizedApiError } from '@/hooks/useLocalizedApiError';
import { useCallback, useState } from 'react';
import { useTranslation } from 'react-i18next';

interface ModelsRoutesPageProps {
  api: GatewayAdminResources;
  refreshRevision?: number;
  onBusyChange?: (busy: boolean) => void;
}

type CatalogTab = 'logical-models' | 'bindings' | 'routes';

export function ModelsRoutesPage({ api, refreshRevision = 0, onBusyChange }: ModelsRoutesPageProps) {
  const { t } = useTranslation('console');
  const apiErrorText = useLocalizedApiError();
  const [tab, setTab] = useState<CatalogTab>('logical-models');
  const { value: editor, setValue: setEditor, opened: editorOpen, afterExit: editorAfterExit } = useOverlayState<Editor>();
  const { value: deleteTarget, setValue: setDeleteTarget, opened: deleteTargetOpen, afterExit: deleteTargetAfterExit } = useOverlayState<DeleteTarget>();
  const [detailTarget, setDetailTarget] = useState<DetailTarget>();
  const [mutationBusy, setMutationBusy] = useState(false);
  const [mutationError, setMutationError] = useState<AdminErrorShape>();
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
    clearOperationNotification();
    setMutationBusy(true);
    setMutationError(undefined);
    onBusyChange?.(true);
    try {
      await operation();
      setEditor(undefined);
      setDeleteTarget(undefined);
      setDetailTarget(undefined);
      notifySuccess(successMessage);
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
      {query.error && <ErrorState error={query.error} onRetry={query.reload} />}
      {mutationError && !editor && !deleteTarget && <ErrorState error={mutationError} />}

      <div role="tabpanel" id="models-tabs-panel" aria-labelledby={`models-tabs-${tab}`} tabIndex={0}>
      {tab === 'logical-models' && (data.logicalModels.length === 0 ? <EmptyTable title={t('models.empty.lm_title')} description={t('models.empty.lm_desc')} /> : (
        <Card variant="flush" title={t('models.card.lm_title')}>
          <TableScroll label={t('models.table.region_logical_models')}><Table className={styles.table}>
            <Table.Thead><Table.Tr><Table.Th scope="col">{t('models.field.lm')}</Table.Th><Table.Th scope="col">{t('models.field.public_name')}</Table.Th><Table.Th scope="col">{t('models.table.header_catalog_status')}</Table.Th><Table.Th scope="col">{t('models.table.header_bindings')}</Table.Th><Table.Th scope="col">{t('models.table.header_routes')}</Table.Th><Table.Th scope="col">{t('models.table.header_enabled')}</Table.Th><Table.Th scope="col">{t('common.actions')}</Table.Th></Table.Tr></Table.Thead>
            <Table.Tbody>{data.logicalModels.map((model) => (
              <Table.Tr key={model.id} data-clickable="true" onClick={() => setDetailTarget({ kind: 'logical-model', record: model })}>
                <Table.Td><span className={styles.primaryText}><strong>{model.display_name}</strong><small><code>{model.id}</code></small></span></Table.Td>
                <Table.Td><code>{model.public_name}</code></Table.Td>
                <Table.Td><StatusPill tone={statusTone(model.status)}>{t(`values.status.${model.status}`, { defaultValue: model.status })}</StatusPill></Table.Td>
                <Table.Td>{data.bindings.filter((binding) => binding.logical_model_id === model.id).length}</Table.Td>
                <Table.Td>{data.routes.filter((route) => route.logical_model_id === model.id).length}</Table.Td>
                <Table.Td onClick={(event) => event.stopPropagation()}><Toggle label={t('models.table.toggle_aria', { id: model.id })} checked={model.enabled} disabled={mutationBusy} onChange={(enabled) => void mutate(() => api.setLogicalModelEnabled(model.id, enabled), t(enabled ? 'models.table.toggle_enabled' : 'models.table.toggle_disabled', { name: model.id }))} /></Table.Td>
                <Table.Td onClick={(event) => event.stopPropagation()}><div className={styles.rowActions}>
                  <IconButton label={t('models.table.view_aria', { id: model.id })} onClick={() => setDetailTarget({ kind: 'logical-model', record: model })}><IconEye size={16} /></IconButton>
                  <IconButton label={t('models.table.edit_aria', { id: model.id })} onClick={() => setEditor({ kind: 'logical-model', record: model })}><IconPencil size={16} /></IconButton>
                  <IconButton label={model.enabled ? t('models.table.disable_aria', { id: model.id }) : t('models.table.enable_aria', { id: model.id })} onClick={() => void mutate(() => api.setLogicalModelEnabled(model.id, !model.enabled), t(model.enabled ? 'models.table.toggle_disabled' : 'models.table.toggle_enabled', { name: model.id }))}><IconPower size={16} /></IconButton>
                  <IconButton label={t('models.table.delete_aria', { id: model.id })} className={styles.dangerIcon} onClick={() => setDeleteTarget({ kind: 'logical-model', record: model })}><IconTrash2 size={16} /></IconButton>
                </div></Table.Td>
              </Table.Tr>
            ))}</Table.Tbody>
          </Table></TableScroll>
        </Card>
      ))}

      {tab === 'bindings' && (data.bindings.length === 0 ? <EmptyTable title={t('models.empty.binding_title')} description={t('models.empty.binding_desc')} /> : (
        <Card variant="flush" title={t('models.card.binding_title')} subtitle={t('models.card.binding_subtitle')}>
          <TableScroll label={t('models.table.region_bindings')}><Table className={`${styles.table} ${styles.bindingsTable}`}>
            <Table.Thead><Table.Tr><Table.Th scope="col">{t('models.field.binding_id')}</Table.Th><Table.Th scope="col">{t('models.field.lm')}</Table.Th><Table.Th scope="col">{t('models.field.source_account')}</Table.Th><Table.Th scope="col">{t('models.field.upstream_model')}</Table.Th><Table.Th scope="col">{t('models.field.protocol')}</Table.Th><Table.Th scope="col">{t('models.field.priority')}</Table.Th><Table.Th scope="col">{t('models.field.runtime')}</Table.Th><Table.Th scope="col">{t('common.status')}</Table.Th><Table.Th scope="col">{t('models.table.header_enabled')}</Table.Th><Table.Th scope="col">{t('common.actions')}</Table.Th></Table.Tr></Table.Thead>
            <Table.Tbody>{sortedBindings.map((binding) => {
              const runtimeCells = resolvedCellsForBinding(data.capabilities, binding.id);
              return (
                <Table.Tr key={binding.id} data-clickable="true" onClick={() => setDetailTarget({ kind: 'binding', record: binding })}>
                  <Table.Td><code>#{binding.id}</code></Table.Td>
                  <Table.Td><code>{binding.logical_model_id}</code></Table.Td>
                  <Table.Td><span className={styles.primaryText}><strong>{binding.source_id}</strong><small>{binding.account_id}</small></span></Table.Td>
                  <Table.Td><code>{binding.upstream_model_id}</code></Table.Td>
                  <Table.Td><ProtocolPill protocol={binding.protocol} /></Table.Td>
                  <Table.Td><strong className={styles.mono}>{binding.priority}</strong></Table.Td>
                  <Table.Td><RuntimeBindingSummary cells={runtimeCells} /></Table.Td>
                  <Table.Td><StatusPill tone={statusTone(binding.status)}>{t(`values.status.${binding.status}`, { defaultValue: binding.status })}</StatusPill></Table.Td>
                  <Table.Td onClick={(event) => event.stopPropagation()}><Toggle label={t('models.table.toggle_aria', { id: binding.id })} checked={binding.enabled} disabled={mutationBusy} onChange={(enabled) => void mutate(() => api.setModelBindingEnabled(binding.id, enabled), t(enabled ? 'models.table.binding_toggle_enabled' : 'models.table.binding_toggle_disabled', { name: binding.id }))} /></Table.Td>
                  <Table.Td onClick={(event) => event.stopPropagation()}><div className={styles.rowActions}>
                    <IconButton label={t('models.table.view_aria', { id: binding.id })} onClick={() => setDetailTarget({ kind: 'binding', record: binding })}><IconEye size={16} /></IconButton>
                    <IconButton label={t('models.table.edit_aria', { id: binding.id })} onClick={() => setEditor({ kind: 'binding', record: binding })}><IconPencil size={16} /></IconButton>
                    <IconButton label={binding.enabled ? t('models.table.disable_aria', { id: binding.id }) : t('models.table.enable_aria', { id: binding.id })} onClick={() => void mutate(() => api.setModelBindingEnabled(binding.id, !binding.enabled), t(binding.enabled ? 'models.table.binding_toggle_disabled' : 'models.table.binding_toggle_enabled', { name: binding.id }))}><IconPower size={16} /></IconButton>
                    <IconButton label={t('models.table.delete_aria', { id: binding.id })} className={styles.dangerIcon} onClick={() => setDeleteTarget({ kind: 'binding', record: binding })}><IconTrash2 size={16} /></IconButton>
                  </div></Table.Td>
                </Table.Tr>
              );
            })}</Table.Tbody>
          </Table></TableScroll>
        </Card>
      ))}

      {tab === 'routes' && (data.routes.length === 0 ? <EmptyTable title={t('models.empty.route_title')} description={t('models.empty.route_desc')} /> : (
        <Card variant="flush" title={t('models.card.route_title')}>
          <TableScroll label={t('models.table.region_routes')}><Table className={`${styles.table} ${styles.routesTable}`}>
            <Table.Thead><Table.Tr><Table.Th scope="col">{t('models.field.route_id')}</Table.Th><Table.Th scope="col">{t('models.field.lm')}</Table.Th><Table.Th scope="col">{t('models.field.protocols')}</Table.Th><Table.Th scope="col">{t('models.field.strategy')}</Table.Th><Table.Th scope="col">{t('models.table.header_runtime_rows')}</Table.Th><Table.Th scope="col">{t('models.field.lossy_value')}</Table.Th><Table.Th scope="col">{t('models.table.header_enabled')}</Table.Th><Table.Th scope="col">{t('common.actions')}</Table.Th></Table.Tr></Table.Thead>
            <Table.Tbody>{data.routes.map((route) => {
              const runtimeRows = data.capabilities.data.filter((row) => row.route_id === route.id);
              const adapterCount = runtimeRows.flatMap((row) => row.protocols).filter((cell) => cell.status === 'routable' && cell.mode === 'adapter').length;
              return (
                <Table.Tr key={route.id} data-clickable="true" onClick={() => setDetailTarget({ kind: 'route', record: route })}>
                  <Table.Td><span className={styles.primaryText}><strong><code>{route.id}</code></strong><small>{route.public_name}</small></span></Table.Td>
                  <Table.Td><code>{route.logical_model_id}</code></Table.Td>
                  <Table.Td><span className={styles.inlineActions}>{route.protocols.map((protocol) => <ProtocolPill key={protocol} protocol={protocol} />)}</span></Table.Td>
                  <Table.Td>{route.strategy === 'primary_then_weighted_fallback'
                    ? <code className={styles.routeStrategy} title={route.strategy}>{t('values.strategy.primary_then_weighted_fallback')}</code>
                    : <StatusPill tone="danger">{t('models.table.runtime_unsupported', { strategy: route.strategy })}</StatusPill>}</Table.Td>
                  <Table.Td><span className={styles.inlineActions}><StatusPill tone={runtimeRows.length > 0 ? 'success' : 'muted'}>{runtimeRows.length > 0 ? `${runtimeRows.length} ${t('models.state.published')}` : t('models.state.not_published')}</StatusPill>{adapterCount > 0 && <StatusPill tone="warning">{t('models.table.runtime_adapter_cells', { count: adapterCount })}</StatusPill>}</span></Table.Td>
                  <Table.Td><StatusPill tone={route.allow_lossy_conversion ? 'warning' : 'muted'}>{route.allow_lossy_conversion ? t('models.state.lossy_allowed') : t('models.state.lossy_blocked')}</StatusPill></Table.Td>
                  <Table.Td onClick={(event) => event.stopPropagation()}><Toggle label={t('models.table.toggle_aria', { id: route.id })} checked={route.enabled} disabled={mutationBusy} onChange={(enabled) => void mutate(() => api.setRouteEnabled(route.id, enabled), t(enabled ? 'models.table.route_toggle_enabled' : 'models.table.route_toggle_disabled', { name: route.id }))} /></Table.Td>
                  <Table.Td onClick={(event) => event.stopPropagation()}><div className={styles.rowActions}>
                    <IconButton label={t('models.table.view_aria', { id: route.id })} onClick={() => setDetailTarget({ kind: 'route', record: route })}><IconEye size={16} /></IconButton>
                    <IconButton label={t('models.table.edit_aria', { id: route.id })} onClick={() => setEditor({ kind: 'route', record: route })}><IconPencil size={16} /></IconButton>
                    <IconButton label={route.enabled ? t('models.table.disable_aria', { id: route.id }) : t('models.table.enable_aria', { id: route.id })} onClick={() => void mutate(() => api.setRouteEnabled(route.id, !route.enabled), t(route.enabled ? 'models.table.route_toggle_disabled' : 'models.table.route_toggle_enabled', { name: route.id }))}><IconPower size={16} /></IconButton>
                    <IconButton label={t('models.table.delete_aria', { id: route.id })} className={styles.dangerIcon} onClick={() => setDeleteTarget({ kind: 'route', record: route })}><IconTrash2 size={16} /></IconButton>
                  </div></Table.Td>
                </Table.Tr>
              );
            })}</Table.Tbody>
          </Table></TableScroll>
        </Card>
      ))}

      </div>
      <Modal
        open={editorOpen}
        onExitTransitionEnd={editorAfterExit}
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
        open={deleteTargetOpen}
        onExitTransitionEnd={deleteTargetAfterExit}
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
