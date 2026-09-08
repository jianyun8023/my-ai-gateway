import { Table } from '@mantine/core';
import { useOverlayState } from '@/components/ui/useOverlayState';
import type {
  Account,
  AccountWriteInput,
  AdminErrorShape,
  GatewayAdminResources,
  ProviderPreset,
  Source,
  SourceCreateInput,
  SourceWriteInput
} from '@/admin-api';
import { GATEWAY_PROTOCOLS, normalizeAdminError } from '@/admin-api';
import { Button } from '@/components/ui/Button';
import { Card } from '@/components/ui/Card';
import { IconButton } from '@/components/ui/IconButton';
import {
  IconEye,
  IconPencil,
  IconPlus,
  IconPower,
  IconRefreshCw,
  IconTrash2
} from '@/components/ui/icons';
import { LoadingState } from '@/components/ui/LoadingState';
import { Modal } from '@/components/ui/Modal';
import { SegmentedTabs } from '@/components/ui/SegmentedTabs';
import { StatusPill } from '@/components/ui/StatusPill';
import { TableScroll } from '@/components/ui/TableScroll';
import styles from '@/features/control-plane/ControlPlane.module.scss';
import { ConfirmDialog, EmptyTable, ErrorState, FormError, PageActions, SuccessNotice, Toggle } from '@/features/control-plane/shared';
import { AccountForm } from '@/features/control-plane/sources/AccountForm';
import { credentialKey, protocolModeKey, protocolModeTone } from '@/features/control-plane/sources/presentation';
import { SourceDetailDrawer } from '@/features/control-plane/sources/SourceDetailDrawer';
import { SourceForm } from '@/features/control-plane/sources/SourceForm';
import { useAdminQuery } from '@/hooks/useAdminQuery';
import { useLocalizedApiError } from '@/hooks/useLocalizedApiError';
import { PROTOCOL_LABELS } from '@/lib/protocols';
import { formatDateTime } from '@/utils/format';
import { useCallback, useState } from 'react';
import { useTranslation } from 'react-i18next';

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

export function SourcesPage({ api, refreshRevision = 0, onBusyChange }: SourcesPageProps) {
  const { t } = useTranslation('console');
  const localizeError = useLocalizedApiError();
  const [tab, setTab] = useState<SourcesTab>('sources');
  const { value: editor, setValue: setEditor, opened: editorOpen, afterExit: editorAfterExit } = useOverlayState<Editor>();
  const { value: deleteTarget, setValue: setDeleteTarget, opened: deleteTargetOpen, afterExit: deleteTargetAfterExit } = useOverlayState<DeleteTarget>();
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
      record
        ? t('sources.message.source_updated', { name: record.id })
        : t('sources.message.source_created', { name: input.id }),
    );
  };

  const submitAccount = (input: AccountWriteInput) => {
    const record = editor?.kind === 'account' ? editor.record : undefined;
    void mutate(
      () => record ? api.updateAccount(record.id, input) : api.createAccount(input),
      record
        ? t('sources.message.account_updated', { name: record.id })
        : t('sources.message.account_created', { name: input.id }),
    );
  };

  const confirmDelete = () => {
    if (!deleteTarget) return;
    void mutate(
      () => deleteTarget.kind === 'source'
        ? api.deleteSource(deleteTarget.record.id)
        : api.deleteAccount(deleteTarget.record.id),
      deleteTarget.kind === 'source'
        ? t('sources.message.source_deleted', { name: deleteTarget.record.id })
        : t('sources.message.account_deleted', { name: deleteTarget.record.id }),
    );
  };

  if (query.loading && !data) return <LoadingState label={t('sources.loading')} />;
  if (query.error && !data) return <ErrorState error={query.error} onRetry={query.reload} />;
  if (!data) return null;

  const formErrorMessage = mutationError ? localizeError(mutationError) : undefined;

  return (
    <section className={styles.page} data-od-id="page-sources">
      <PageActions>
        <SegmentedTabs
          id="sources-tabs"
          value={tab}
          label={t('sources.region_aria')}
          options={[
            { value: 'sources', label: t('sources.tab.sources'), count: data.sources.length },
            { value: 'accounts', label: t('sources.tab.accounts'), count: data.accounts.length },
          ]}
          onChange={setTab}
        />
        <div className={styles.rowActions}>
          <Button variant="secondary" onClick={query.reload} loading={query.refreshing}><IconRefreshCw size={14} />{t('common.refresh')}</Button>
          <Button variant="primary" onClick={() => setEditor(tab === 'sources' ? { kind: 'source' } : { kind: 'account' })} disabled={tab === 'accounts' && data.sources.length === 0}>
            <IconPlus size={14} />{tab === 'sources' ? t('sources.add_source') : t('sources.add_account')}
          </Button>
        </div>
      </PageActions>

      <SuccessNotice message={notice} onDismiss={() => setNotice('')} />
      {query.error && <ErrorState error={query.error} onRetry={query.reload} />}
      {mutationError && !editor && !deleteTarget && <ErrorState error={mutationError} />}

      <div role="tabpanel" id="sources-tabs-panel" aria-labelledby={`sources-tabs-${tab}`} tabIndex={0}>
      {tab === 'sources' ? (
        data.sources.length === 0 ? <EmptyTable title={t('sources.empty.sources_title')} description={t('sources.empty.sources_desc')} /> : (
          <Card variant="flush" title={t('sources.card.sources_title')}>
            <TableScroll label={t('sources.table.sources_region')}>
              <Table className={styles.table}>
                <Table.Thead><Table.Tr>
                  <Table.Th scope="col">{t('sources.field.source')}</Table.Th>
                  <Table.Th scope="col">{t('sources.field.provider_preset')}</Table.Th>
                  <Table.Th scope="col">{t('sources.field.base_url')}</Table.Th>
                  {GATEWAY_PROTOCOLS.map((protocol) => <Table.Th scope="col" key={protocol}>{PROTOCOL_LABELS[protocol]}</Table.Th>)}
                  <Table.Th scope="col">{t('sources.tab.accounts')}</Table.Th>
                  <Table.Th scope="col">{t('common.status')}</Table.Th>
                  <Table.Th scope="col">{t('common.actions')}</Table.Th>
                </Table.Tr></Table.Thead>
                <Table.Tbody>{data.sources.map((source) => (
                  <Table.Tr key={source.id} data-clickable="true" onClick={() => setSelectedSourceId(source.id)}>
                    <Table.Td><span className={styles.primaryText}><strong>{source.display_name}</strong><small className={styles.mono}>{source.id}</small></span></Table.Td>
                    <Table.Td><code>{source.provider_preset_id}@{source.provider_preset_version}</code></Table.Td>
                    <Table.Td><code>{source.base_url}</code></Table.Td>
                    {GATEWAY_PROTOCOLS.map((protocol) => (
                      <Table.Td key={protocol}><StatusPill tone={protocolModeTone(source.protocol_capabilities[protocol]?.mode)}>{t(protocolModeKey(source.protocol_capabilities[protocol]?.mode))}</StatusPill></Table.Td>
                    ))}
                    <Table.Td><span className={styles.mono}>{data.accounts.filter((account) => account.source_id === source.id).length}</span></Table.Td>
                    <Table.Td onClick={(event) => event.stopPropagation()}><Toggle label={t('sources.table.toggle_aria', { id: source.id })} checked={source.enabled} disabled={mutationBusy} onChange={(enabled) => void mutate(() => api.setSourceEnabled(source.id, enabled), t(enabled ? 'sources.table.toggle_enabled' : 'sources.table.toggle_disabled', { name: source.id }))} /></Table.Td>
                    <Table.Td onClick={(event) => event.stopPropagation()}><div className={styles.rowActions}>
                      <IconButton label={t('sources.table.view_aria', { id: source.id })} onClick={() => setSelectedSourceId(source.id)}><IconEye size={16} /></IconButton>
                      <IconButton label={t('sources.table.edit_aria', { id: source.id })} onClick={() => setEditor({ kind: 'source', record: source })}><IconPencil size={16} /></IconButton>
                      <IconButton label={`${source.enabled ? t('common.disable') : t('common.enable')} ${source.id}`} disabled={mutationBusy} onClick={() => void mutate(() => api.setSourceEnabled(source.id, !source.enabled), t(source.enabled ? 'sources.table.toggle_disabled' : 'sources.table.toggle_enabled', { name: source.id }))}><IconPower size={16} /></IconButton>
                      <IconButton label={t('sources.table.delete_aria', { id: source.id })} className={styles.dangerIcon} onClick={() => setDeleteTarget({ kind: 'source', record: source })}><IconTrash2 size={16} /></IconButton>
                    </div></Table.Td>
                  </Table.Tr>
                ))}</Table.Tbody>
              </Table>
            </TableScroll>
          </Card>
        )
      ) : data.accounts.length === 0 ? <EmptyTable title={t('sources.empty.accounts_title')} description={t('sources.empty.accounts_desc')} /> : (
        <Card variant="flush" title={t('sources.card.accounts_title')}>
          <TableScroll label={t('sources.table.accounts_region')}>
            <Table className={styles.table}>
              <Table.Thead><Table.Tr>
                <Table.Th scope="col">{t('common.account')}</Table.Th>
                <Table.Th scope="col">{t('sources.field.source')}</Table.Th>
                <Table.Th scope="col">{t('sources.table.header_credentials')}</Table.Th>
                <Table.Th scope="col">{t('sources.table.header_fallback_weight')}</Table.Th>
                <Table.Th scope="col">{t('sources.table.header_health')}</Table.Th>
                <Table.Th scope="col">{t('sources.table.header_cooldown')}</Table.Th>
                <Table.Th scope="col">{t('common.status')}</Table.Th>
                <Table.Th scope="col">{t('common.actions')}</Table.Th>
              </Table.Tr></Table.Thead>
              <Table.Tbody>{data.accounts.map((account) => (
                <Table.Tr key={account.id}>
                  <Table.Td><span className={styles.primaryText}><strong>{account.display_name}</strong><small className={styles.mono}>{account.id}</small></span></Table.Td>
                  <Table.Td><code>{account.source_id}</code></Table.Td>
                  <Table.Td><StatusPill tone={account.credential_configured ? 'success' : 'danger'}>{t(credentialKey(account))}</StatusPill></Table.Td>
                  <Table.Td><span className={styles.mono}>{account.weight}</span></Table.Td>
                  <Table.Td><StatusPill tone={account.health_status === 'healthy' ? 'success' : account.health_status === 'unknown' ? 'accent' : 'warning'}>{t(`values.health.${account.health_status || 'unknown'}`, { defaultValue: account.health_status || 'unknown' })}</StatusPill></Table.Td>
                  <Table.Td>{formatDateTime(account.cooldown_until)}</Table.Td>
                  <Table.Td><Toggle label={t('sources.table.toggle_aria', { id: account.id })} checked={account.enabled} disabled={mutationBusy} onChange={(enabled) => void mutate(() => api.setAccountEnabled(account.id, enabled), t(enabled ? 'sources.table.account_toggle_enabled' : 'sources.table.account_toggle_disabled', { name: account.id }))} /></Table.Td>
                  <Table.Td><div className={styles.rowActions}>
                    <IconButton label={t('sources.table.edit_aria', { id: account.id })} onClick={() => setEditor({ kind: 'account', record: account })}><IconPencil size={16} /></IconButton>
                    <IconButton label={`${account.enabled ? t('common.disable') : t('common.enable')} ${account.id}`} disabled={mutationBusy} onClick={() => void mutate(() => api.setAccountEnabled(account.id, !account.enabled), t(account.enabled ? 'sources.table.account_toggle_disabled' : 'sources.table.account_toggle_enabled', { name: account.id }))}><IconPower size={16} /></IconButton>
                    <IconButton label={t('sources.table.delete_aria', { id: account.id })} className={styles.dangerIcon} onClick={() => setDeleteTarget({ kind: 'account', record: account })}><IconTrash2 size={16} /></IconButton>
                  </div></Table.Td>
                </Table.Tr>
              ))}</Table.Tbody>
            </Table>
          </TableScroll>
        </Card>
      )}

      </div>
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
        open={editorOpen}
        onExitTransitionEnd={editorAfterExit}
        title={editor?.kind === 'source'
          ? editor.record ? t('sources.modal.edit_source') : t('sources.modal.new_source')
          : editor?.record ? t('sources.modal.edit_account') : t('sources.modal.new_account')}
        onClose={() => !mutationBusy && setEditor(undefined)}
        closeDisabled={mutationBusy}
        width={680}
        footer={editor && (
          <>
            <Button variant="secondary" onClick={() => setEditor(undefined)} disabled={mutationBusy}>{t('common.cancel')}</Button>
            <Button type="submit" form={editor.kind === 'source' ? 'source-editor-form' : 'account-editor-form'} loading={mutationBusy}>
              {editor.record ? t('common.save_changes') : t('common.create')}
            </Button>
          </>
        )}
      >
        {editor?.kind === 'source' && <SourceForm key={editor.record?.id ?? 'new-source'} record={editor.record} presets={data.presets} busy={mutationBusy} error={formErrorMessage} onSubmit={submitSource} />}
        {editor?.kind === 'account' && <AccountForm key={editor.record?.id ?? 'new-account'} record={editor.record} sources={data.sources} busy={mutationBusy} error={formErrorMessage} onSubmit={submitAccount} />}
      </Modal>

      <ConfirmDialog
        open={deleteTargetOpen}
        onExitTransitionEnd={deleteTargetAfterExit}
        title={deleteTarget?.kind === 'source' ? t('sources.confirm.delete_source_title') : t('sources.confirm.delete_account_title')}
        description={deleteTarget ? <div className={styles.page}>{t(deleteTarget.kind === 'source' ? 'sources.confirm.delete_source_body' : 'sources.confirm.delete_account_body', { id: deleteTarget.record.id })}<FormError message={formErrorMessage} /></div> : null}
        confirmLabel={t('common.delete')}
        danger
        busy={mutationBusy}
        onCancel={() => !mutationBusy && setDeleteTarget(undefined)}
        onConfirm={confirmDelete}
      />
    </section>
  );
}
