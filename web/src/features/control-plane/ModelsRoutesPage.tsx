import type { AdminErrorShape, GatewayAdminResources, LogicalModel, ModelRoutingWriteInput } from '@/admin-api';
import { normalizeAdminError } from '@/admin-api';
import { Table } from '@mantine/core';
import { Button } from '@/components/ui/Button';
import { Card } from '@/components/ui/Card';
import { TextField } from '@/components/ui/FormField';
import { FormActions } from '@/components/ui/FormActions';
import { IconButton } from '@/components/ui/IconButton';
import { IconPencil, IconPlus, IconRefreshCw, IconTrash2 } from '@/components/ui/icons';
import { LoadingState } from '@/components/ui/LoadingState';
import { Modal } from '@/components/ui/Modal';
import { clearOperationNotification, notifySuccess } from '@/components/ui/notifications';
import { Popover } from '@/components/ui/overlays';
import { StatusPill, type StatusTone } from '@/components/ui/StatusPill';
import { TableScroll } from '@/components/ui/TableScroll';
import { useOverlayState } from '@/components/ui/useOverlayState';
import styles from '@/features/control-plane/ControlPlane.module.scss';
import pageStyles from '@/features/control-plane/models/ModelsRoutes.module.scss';
import type { CatalogData } from '@/features/control-plane/models/catalog';
import { ModelRoutePath } from '@/features/control-plane/models/ModelRoutePath';
import { ModelRoutingEditor } from '@/features/control-plane/models/ModelRoutingEditor';
import { summarizeModelRouting } from '@/features/control-plane/models/routingPresentation';
import { ConfirmDialog, EmptyTable, ErrorState, PageActions, ProtocolPill, Toggle } from '@/features/control-plane/shared';
import { useAdminQuery } from '@/hooks/useAdminQuery';
import { useLocalizedApiError } from '@/hooks/useLocalizedApiError';
import { useCallback, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';

interface ModelsRoutesPageProps {
  api: GatewayAdminResources;
  refreshRevision?: number;
  onBusyChange?: (busy: boolean) => void;
}

const statusTones: Record<string, StatusTone> = {
  healthy: 'success', degraded: 'warning', unavailable: 'danger',
  disabled: 'muted', pending: 'warning', unknown: 'muted',
};

export function ModelsRoutesPage({ api, refreshRevision = 0, onBusyChange }: ModelsRoutesPageProps) {
  const { t } = useTranslation('console');
  const apiErrorText = useLocalizedApiError();
  const [search, setSearch] = useState('');
  const editor = useOverlayState<{ id?: string }>();
  const deletion = useOverlayState<LogicalModel>();
  const mutationLock = useRef(false);
  const [mutationBusy, setMutationBusy] = useState(false);
  const [mutationError, setMutationError] = useState<AdminErrorShape>();

  const load = useCallback(async (signal: AbortSignal): Promise<CatalogData> => {
    const [logicalModels, bindings, routes, sources, accounts, capabilities] = await Promise.all([
      api.logicalModels(signal), api.modelBindings(signal), api.routes(signal),
      api.sources(signal), api.accounts(signal), api.capabilities(signal),
    ]);
    return { logicalModels, bindings, routes, sources, accounts, capabilities };
  }, [api]);
  const query = useAdminQuery({ load, refreshRevision, onBusyChange });
  const data = query.data;

  const mutate = async (operation: () => Promise<unknown>, successMessage: string) => {
    if (mutationLock.current) return;
    mutationLock.current = true;
    clearOperationNotification();
    setMutationBusy(true);
    setMutationError(undefined);
    onBusyChange?.(true);
    try {
      await operation();
      editor.setValue(undefined);
      deletion.setValue(undefined);
      notifySuccess(successMessage);
      query.reload();
    } catch (error) {
      setMutationError(normalizeAdminError(error));
    } finally {
      mutationLock.current = false;
      setMutationBusy(false);
      onBusyChange?.(false);
    }
  };

  const openEditor = (id?: string) => {
    setMutationError(undefined);
    editor.setValue({ id });
  };
  const openDelete = (model: LogicalModel) => {
    setMutationError(undefined);
    deletion.setValue(model);
  };
  const submit = (id: string, input: ModelRoutingWriteInput) => {
    void mutate(() => api.saveModelRouting(id, input), t('models.v3.saved'));
  };

  if (query.loading && !data) return <LoadingState label={t('models.loading')} />;
  if (query.error && !data) return <ErrorState error={query.error} onRetry={query.reload} />;
  if (!data) return null;

  const searchText = search.trim().toLocaleLowerCase();
  const models = data.logicalModels.filter((model) =>
    [model.public_name, model.display_name].some((value) => value.toLocaleLowerCase().includes(searchText)));

  return (
    <section className={styles.page} data-od-id="page-models-routes">
      <PageActions>
        <TextField label={t('models.v3.search')} value={search} onChange={(event) => setSearch(event.target.value)} className={pageStyles.search} type="search" />
        <div className={styles.rowActions}>
          <Button variant="secondary" onClick={query.reload} loading={query.refreshing}><IconRefreshCw size={14} />{t('common.refresh')}</Button>
          <Button onClick={() => openEditor()} disabled={mutationBusy}><IconPlus size={14} />{t('models.v3.create')}</Button>
        </div>
      </PageActions>
      {query.error && <ErrorState error={query.error} onRetry={query.reload} />}
      {mutationError && !editor.opened && !deletion.opened && <ErrorState error={mutationError} />}
      {data.logicalModels.length === 0 ? <EmptyTable title={t('models.empty.lm_title')} description={t('models.empty.lm_desc')} />
        : models.length === 0 ? <EmptyTable title={t('models.v3.empty_search')} /> : (
          <Card variant="flush" title={t('models.v3.list_title')}>
            <TableScroll label={t('models.v3.list_title')}><Table className={pageStyles.table}>
              <Table.Thead><Table.Tr>
                <Table.Th scope="col">{t('models.field.lm')}</Table.Th>
                <Table.Th scope="col">{t('models.field.protocols')}</Table.Th>
                <Table.Th scope="col">{t('models.v3.lines')}</Table.Th>
                <Table.Th scope="col">{t('common.status')}</Table.Th>
                <Table.Th scope="col">{t('common.actions')}</Table.Th>
              </Table.Tr></Table.Thead>
              <Table.Tbody>{models.map((model) => {
                const summary = summarizeModelRouting(model, data);
                return <Table.Tr key={model.id} data-clickable="true" onClick={() => openEditor(model.id)}>
                  <Table.Td><span className={styles.primaryText}><strong>{model.public_name}</strong>{model.display_name !== model.public_name && <small>{model.display_name}</small>}</span></Table.Td>
                  <Table.Td><div className={pageStyles.protocols}>{summary.protocols.map((protocol) => (
                    <span key={protocol.protocol}>
                      <ProtocolPill protocol={protocol.protocol} />
                      <StatusPill tone={protocol.mode === 'native' ? 'success' : protocol.mode === 'adapter' || protocol.mode === 'mixed' ? 'warning' : 'muted'}>{t(`models.v3.${protocol.mode}`)}</StatusPill>
                    </span>
                  ))}</div></Table.Td>
                  <Table.Td><ModelRoutePath summary={summary} /></Table.Td>
                  <Table.Td><StatusPill tone={statusTones[summary.status]}>{t(`models.v3.${summary.status}`)}</StatusPill></Table.Td>
                  <Table.Td onClick={(event) => event.stopPropagation()}>
                    <div className={pageStyles.actions}>
                      <Toggle label={t('models.table.toggle_aria', { id: model.public_name })} checked={model.enabled} disabled={mutationBusy} showLabel={false} onChange={(enabled) => void mutate(() => api.setLogicalModelEnabled(model.id, enabled), t(enabled ? 'models.table.toggle_enabled' : 'models.table.toggle_disabled', { name: model.public_name }))} />
                      <IconButton label={t('models.table.edit_aria', { id: model.public_name })} disabled={mutationBusy} onClick={() => openEditor(model.id)}><IconPencil size={16} /></IconButton>
                      <ModelMoreActions name={model.public_name} busy={mutationBusy} onDelete={() => openDelete(model)} />
                    </div>
                  </Table.Td>
                </Table.Tr>;
              })}</Table.Tbody>
            </Table></TableScroll>
          </Card>
        )}
      {editor.value && <ModelRoutingDrawer
        key={editor.value.id ?? 'new'} id={editor.value.id} open={editor.opened} onClose={() => editor.setValue(undefined)} afterExit={editor.afterExit}
        api={api} data={data} busy={mutationBusy} error={mutationError ? apiErrorText(mutationError) : undefined} onSubmit={submit}
      />}
      <ConfirmDialog open={deletion.opened} onExitTransitionEnd={deletion.afterExit} title={t('models.confirm.delete_lm_title')}
        description={<>{t('models.confirm.delete_body', { id: deletion.value?.public_name })}{mutationError && <ErrorState error={mutationError} />}</>}
        confirmLabel={t('common.delete')} danger busy={mutationBusy} onCancel={() => deletion.setValue(undefined)}
        onConfirm={() => deletion.value && void mutate(() => api.deleteLogicalModel(deletion.value!.id), t('models.message.lm_deleted', { name: deletion.value.public_name }))}
      />
    </section>
  );
}

function ModelMoreActions({ name, busy, onDelete }: { name: string; busy: boolean; onDelete: () => void }) {
  const { t } = useTranslation('console');
  const [opened, setOpened] = useState(false);
  return <Popover opened={opened} onChange={setOpened} position="bottom-end" trapFocus>
    <Popover.Target><IconButton label={`${t('models.v3.more_actions')} ${name}`} disabled={busy} onClick={() => setOpened((value) => !value)} aria-expanded={opened}><span aria-hidden="true">⋯</span></IconButton></Popover.Target>
    <Popover.Dropdown inert={!opened}>
      <Button variant="danger" size="sm" disabled={busy} onClick={() => { setOpened(false); onDelete(); }}><IconTrash2 size={14} />{t('common.delete')}</Button>
    </Popover.Dropdown>
  </Popover>;
}

function ModelRoutingDrawer({ id, open, onClose, afterExit, api, data, busy, error, onSubmit }: {
  id?: string; open: boolean; onClose: () => void; afterExit: () => void;
  api: GatewayAdminResources; data: CatalogData; busy: boolean; error?: string;
  onSubmit: (id: string, input: ModelRoutingWriteInput) => void;
}) {
  const { t } = useTranslation('console');
  const load = useCallback((signal: AbortSignal) => id ? api.modelRouting(id, signal) : Promise.resolve(undefined), [api, id]);
  const query = useAdminQuery({ load, queryKey: id ?? 'new' });
  const ready = !id || Boolean(query.data);
  return <Modal open={open} variant="drawer" width={680} title={t(id ? 'models.v3.edit' : 'models.v3.create')}
    onClose={onClose} onExitTransitionEnd={afterExit} closeDisabled={busy}
    footer={ready ? <FormActions form="model-routing-editor-form" cancelLabel={t('common.cancel')} submitLabel={t('common.save')} busy={busy} onCancel={onClose} />
      : <Button variant="secondary" onClick={onClose}>{t('common.close')}</Button>}>
    {query.error && <ErrorState error={query.error} onRetry={query.reload} />}
    {ready ? <ModelRoutingEditor api={api} data={data} configuration={query.data} busy={busy} error={error} onSubmit={onSubmit} />
      : query.loading && <LoadingState label={t('models.loading')} />}
  </Modal>;
}
