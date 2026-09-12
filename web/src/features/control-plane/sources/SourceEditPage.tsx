import { FormActions } from '@/components/ui/FormActions';
import { clearOperationNotification, notifySuccess } from '@/components/ui/notifications';
import { DetailItem, DetailList } from '@/components/ui/DetailList';
import { Table } from '@mantine/core';
import { useOverlayState } from '@/components/ui/useOverlayState';
import type {
  Account,
  AccountWriteInput,
  AdminErrorShape,
  ConnectionTestResult,
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
  IconPencil,
  IconPlay,
  IconPlus,
  IconTrash2,
} from '@/components/ui/icons';
import { LoadingState } from '@/components/ui/LoadingState';
import { Modal } from '@/components/ui/Modal';
import { Notice } from '@/components/ui/Notice';
import { StatusPill } from '@/components/ui/StatusPill';
import { TableScroll } from '@/components/ui/TableScroll';
import styles from '@/features/control-plane/ControlPlane.module.scss';
import { AccountForm } from '@/features/control-plane/sources/AccountForm';
import {
  credentialKey,
  protocolModeKey,
  protocolModeTone,
} from '@/features/control-plane/sources/presentation';
import { SourceForm } from '@/features/control-plane/sources/SourceForm';
import { ConfirmDialog, EmptyTable, ErrorState, FormError, PageActions, Toggle } from '@/features/control-plane/shared';
import { useAdminQuery } from '@/hooks/useAdminQuery';
import { useLocalizedApiError } from '@/hooks/useLocalizedApiError';
import { PROTOCOL_LABELS } from '@/lib/protocols';
import { sourceRouteHash, type SourceSection } from '@/lib/consoleNavigation';
import { formatDateTime } from '@/utils/format';
import { useCallback, useState } from 'react';
import { useTranslation } from 'react-i18next';

interface SourceEditPageProps {
  api: GatewayAdminResources;
  refreshRevision?: number;
  onBusyChange?: (busy: boolean) => void;
  /** 缺省表示新增来源。 */
  sourceId?: string;
  onOpenSource?: (sourceId: string, section?: SourceSection) => void;
}

interface EditContext {
  source?: Source;
  accounts: Account[];
  presets: ProviderPreset[];
}

export function SourceEditPage({ api, refreshRevision = 0, onBusyChange, sourceId, onOpenSource }: SourceEditPageProps) {
  const { t } = useTranslation('console');
  const localizeError = useLocalizedApiError();
  const isCreate = !sourceId;
  const { value: accountEditor, setValue: setAccountEditor, opened: accountEditorOpen, afterExit: accountEditorAfterExit } = useOverlayState<{ record?: Account }>();
  const { value: deleteTarget, setValue: setDeleteTarget, opened: deleteTargetOpen, afterExit: deleteTargetAfterExit } = useOverlayState<Account>();
  const [sourceDeleteOpen, setSourceDeleteOpen] = useState(false);
  const [draft, setDraft] = useState<{ enabled: boolean; capabilities: Source['protocol_capabilities'] }>();
  const [mutationBusy, setMutationBusy] = useState(false);
  const [mutationError, setMutationError] = useState<AdminErrorShape>();
  const [testBusy, setTestBusy] = useState(false);
  const [testResult, setTestResult] = useState<ConnectionTestResult>();

  const openSource = (targetId: string, section?: SourceSection) => {
    if (onOpenSource) {
      onOpenSource(targetId, section);
      return;
    }
    window.location.hash = sourceRouteHash(targetId, section);
  };

  const load = useCallback(async (signal: AbortSignal): Promise<EditContext> => {
    const [sources, accounts, presets] = await Promise.all([
      api.sources(signal),
      api.accounts(signal),
      api.providerPresets(signal),
    ]);
    return { source: sources.find((item) => item.id === sourceId), accounts, presets };
  }, [api, sourceId]);
  const query = useAdminQuery({ load, refreshRevision, onBusyChange });
  const data = query.data;
  const source = data?.source;
  const sourceAccounts = (data?.accounts ?? []).filter((account) => account.source_id === sourceId);
  const enabledAccounts = sourceAccounts.filter((account) => account.enabled);
  const formErrorMessage = mutationError ? localizeError(mutationError) : undefined;

  const mutate = async (operation: () => Promise<unknown>, successMessage: string, after?: () => void) => {
    if (mutationBusy) return;
    clearOperationNotification();
    setMutationBusy(true);
    setMutationError(undefined);
    onBusyChange?.(true);
    try {
      await operation();
      notifySuccess(successMessage);
      query.reload();
      after?.();
    } catch (error) {
      setMutationError(normalizeAdminError(error));
    } finally {
      setMutationBusy(false);
      onBusyChange?.(false);
    }
  };

  const submitSource = (input: SourceCreateInput | SourceWriteInput) => {
    if (isCreate) {
      void mutate(
        () => api.createSource(input as SourceCreateInput),
        t('sources.message.source_created', { name: input.id }),
        () => openSource(input.id, 'edit'),
      );
      return;
    }
    void mutate(
      () => api.updateSource(sourceId!, input as SourceWriteInput),
      t('sources.message.source_updated', { name: sourceId }),
      () => openSource(sourceId!),
    );
  };

  const submitAccount = (input: AccountWriteInput) => {
    const record = accountEditor?.record;
    void mutate(
      () => record ? api.updateAccount(record.id, input) : api.createAccount(input),
      record
        ? t('sources.message.account_updated', { name: record.id })
        : t('sources.message.account_created', { name: input.id }),
      () => setAccountEditor(undefined),
    );
  };

  const confirmDeleteAccount = () => {
    if (!deleteTarget) return;
    void mutate(
      () => api.deleteAccount(deleteTarget.id),
      t('sources.message.account_deleted', { name: deleteTarget.id }),
      () => setDeleteTarget(undefined),
    );
  };

  const confirmDeleteSource = () => {
    if (isCreate || !sourceId) return;
    void mutate(
      () => api.deleteSource(sourceId),
      t('sources.message.source_deleted', { name: sourceId }),
      () => openSource(''),
    );
  };

  const runConnectionTest = async () => {
    const account = enabledAccounts[0];
    if (!account || testBusy) return;
    setTestBusy(true);
    setMutationError(undefined);
    try {
      const result = await api.testConnection(sourceId!, {
        account_id: account.id,
        protocol: 'openai_chat_completions',
        requested_by: 'admin-ui',
      });
      setTestResult(result);
    } catch (error) {
      setMutationError(normalizeAdminError(error));
    } finally {
      setTestBusy(false);
    }
  };

  if (query.loading && !data) return <LoadingState label={t('sources.loading')} />;
  if (query.error && !data) return <ErrorState error={query.error} onRetry={query.reload} />;
  if (data && !isCreate && !source) {
    return (
      <section className={styles.page} data-od-id="page-source-edit">
        <EmptyTable title={t('sources.detail.missing_title', { id: sourceId })} description={t('sources.detail.missing_desc')} />
        {query.error && <ErrorState error={query.error} onRetry={query.reload} />}
        <div className={styles.cardActions}>
          <Button variant="secondary" onClick={() => openSource('')}>{t('sources.detail.back_to_list')}</Button>
        </div>
      </section>
    );
  }
  if (!data) return null;

  return (
    <section className={styles.page} data-od-id="page-source-edit">
      <div className={styles.breadcrumbBar}>
        <Button size="sm" variant="ghost" onClick={() => openSource('')}>{t('sources.detail.back_to_list')}</Button>
        <span aria-hidden="true">/</span>
        {!isCreate && <><Button size="sm" variant="ghost" onClick={() => openSource(sourceId!)}>{source?.display_name ?? sourceId}</Button><span aria-hidden="true">/</span></>}
        <strong>{isCreate ? t('sources.modal.new_source') : t('sources.modal.edit_source')}</strong>
      </div>

      <PageActions>
        <span className={styles.secondaryText}>{t('sources.edit.subtitle')}</span>
        <div className={styles.rowActions}>
          {!isCreate && (
            <Button variant="secondary" loading={testBusy} disabled={enabledAccounts.length === 0} onClick={() => void runConnectionTest()}>
              <IconPlay size={14} />{t('sources.detail.test_connection')}
            </Button>
          )}
          <FormActions
            form="source-editor-form"
            cancelLabel={t('common.cancel')}
            submitLabel={isCreate ? t('common.create') : t('common.save_changes')}
            busy={mutationBusy}
            onCancel={() => openSource(isCreate ? '' : sourceId!)}
          />
          {!isCreate && (
            <Button variant="danger" disabled={mutationBusy} onClick={() => setSourceDeleteOpen(true)}>
              <IconTrash2 size={14} />{t('common.delete')}
            </Button>
          )}
        </div>
      </PageActions>

      {query.error && <ErrorState error={query.error} onRetry={query.reload} />}
      {mutationError && !accountEditorOpen && !deleteTargetOpen && !sourceDeleteOpen && <ErrorState error={mutationError} />}

      <div className={styles.editLayout}>
        <div className={styles.stack}>
          <SourceForm
            key={source?.id ?? 'new-source'}
            record={source}
            presets={data.presets}
            busy={mutationBusy}
            error={formErrorMessage}
            onSubmit={submitSource}
            onDraftChange={setDraft}
          />

          {!isCreate && (
            <Card
              variant="flush"
              title={t('sources.edit.accounts_card')}
              extra={<Button size="sm" variant="secondary" onClick={() => setAccountEditor({})}><IconPlus size={14} />{t('sources.add_account')}</Button>}
            >
              {sourceAccounts.length === 0 ? <EmptyTable title={t('sources.edit.accounts_empty')} description={t('sources.edit.accounts_empty_desc')} /> : (
                <TableScroll label={t('sources.table.accounts_region')}>
                  <Table className={styles.table}>
                    <Table.Thead><Table.Tr>
                      <Table.Th scope="col">{t('common.account')}</Table.Th>
                      <Table.Th scope="col">{t('sources.field.fallback_weight')}</Table.Th>
                      <Table.Th scope="col">{t('sources.table.header_credentials')}</Table.Th>
                      <Table.Th scope="col">{t('sources.table.header_health')}</Table.Th>
                      <Table.Th scope="col">{t('common.status')}</Table.Th>
                      <Table.Th scope="col">{t('common.actions')}</Table.Th>
                    </Table.Tr></Table.Thead>
                    <Table.Tbody>{sourceAccounts.map((account) => (
                      <Table.Tr key={account.id}>
                        <Table.Td><span className={styles.primaryText}><strong>{account.display_name}</strong><small className={styles.mono}>{account.id}</small></span></Table.Td>
                        <Table.Td><span className={styles.mono}>{account.weight}</span></Table.Td>
                        <Table.Td><StatusPill tone={account.credential_configured ? 'success' : 'muted'}>{t(credentialKey(account))}</StatusPill></Table.Td>
                        <Table.Td><StatusPill tone={account.health_status === 'healthy' ? 'success' : account.health_status === 'unknown' ? 'accent' : 'warning'}>{t(`values.health.${account.health_status || 'unknown'}`, { defaultValue: account.health_status || 'unknown' })}</StatusPill></Table.Td>
                        <Table.Td onClick={(event) => event.stopPropagation()}>
                          <Toggle
                            label={t('sources.table.toggle_aria', { id: account.id })}
                            checked={account.enabled}
                            disabled={mutationBusy}
                            onChange={() => void mutate(
                              () => api.setAccountEnabled(account.id, !account.enabled),
                              t(account.enabled ? 'sources.table.account_toggle_disabled' : 'sources.table.account_toggle_enabled', { name: account.id }),
                            )}
                          />
                        </Table.Td>
                        <Table.Td onClick={(event) => event.stopPropagation()}><div className={styles.rowActions}>
                          <IconButton label={t('sources.table.edit_aria', { id: account.id })} onClick={() => setAccountEditor({ record: account })}><IconPencil size={16} /></IconButton>
                          <IconButton label={t('sources.table.delete_aria', { id: account.id })} className={styles.dangerIcon} onClick={() => setDeleteTarget(account)}><IconTrash2 size={16} /></IconButton>
                        </div></Table.Td>
                      </Table.Tr>
                    ))}</Table.Tbody>
                  </Table>
                </TableScroll>
              )}
            </Card>
          )}

          <Card title={t('sources.edit.sync_card')}>
            <Notice tone="warning">
              <span>{t('sources.edit.sync_desc')}</span>
            </Notice>
          </Card>
        </div>

        <div className={styles.stack}>
          <Card title={t('sources.edit.summary_card')}>
            <DetailList>
              <DetailItem label={t('sources.edit.summary_status')}>
                <StatusPill tone={(draft?.enabled ?? source?.enabled) ? 'success' : 'muted'}>{source || draft ? t((draft?.enabled ?? source?.enabled) ? 'common.enabled' : 'common.disabled') : t('sources.edit.summary_new')}</StatusPill>
              </DetailItem>
              <DetailItem label={t('sources.edit.summary_accounts')}>{isCreate ? '—' : t('sources.edit.summary_accounts_value', { count: sourceAccounts.length })}</DetailItem>
              <DetailItem label={t('sources.edit.summary_protocols')}>
                <span className={styles.inlineActions}>
                  {GATEWAY_PROTOCOLS.map((protocol) => {
                    const capabilities = draft?.capabilities ?? source?.protocol_capabilities;
                    return <StatusPill key={protocol} tone={protocolModeTone(capabilities?.[protocol]?.mode)} title={PROTOCOL_LABELS[protocol]}>{t(protocolModeKey(capabilities?.[protocol]?.mode))}</StatusPill>;
                  })}
                </span>
              </DetailItem>
              <DetailItem label={t('common.updated_at')}>{source ? formatDateTime(source.updated_at) : '—'}</DetailItem>
            </DetailList>
          </Card>

          <Card title={t('sources.edit.flow_card')}>
            <ol className={styles.eventList}>
              {[1, 2, 3, 4].map((step) => (
                <li key={step}>
                  <strong>{t(`sources.edit.flow_step_${step}`)}</strong>
                  <small>{t(`sources.edit.flow_step_${step}_desc`)}</small>
                </li>
              ))}
            </ol>
          </Card>

          {!isCreate && (
            <Card title={t('sources.edit.verify_card')}>
              <DetailList>
                <DetailItem label={t('sources.edit.verify_health')}>
                  {enabledAccounts[0]
                    ? <StatusPill tone={enabledAccounts[0].health_status === 'healthy' ? 'success' : enabledAccounts[0].health_status === 'unknown' ? 'accent' : 'warning'}>{t(`values.health.${enabledAccounts[0].health_status || 'unknown'}`, { defaultValue: enabledAccounts[0].health_status || 'unknown' })}</StatusPill>
                    : <span className={styles.secondaryText}>—</span>}
                </DetailItem>
                <DetailItem label={t('sources.edit.verify_connection')}>
                  {testResult
                    ? <StatusPill tone={testResult.status === 'succeeded' ? 'success' : 'danger'}>{testResult.status === 'succeeded' ? t('sources.edit.verify_ok', { latency: testResult.latency_ms }) : t('sources.edit.verify_failed')}</StatusPill>
                    : <span className={styles.secondaryText}>{t('sources.edit.verify_none')}</span>}
                </DetailItem>
                <DetailItem label={t('sources.edit.verify_probe')}>
                  {enabledAccounts[0]?.last_probe_at ? formatDateTime(enabledAccounts[0].last_probe_at) : t('sources.edit.verify_none')}
                </DetailItem>
              </DetailList>
              <p className={styles.secondaryText}>{t('sources.edit.verify_saved_hint')}</p>
            </Card>
          )}
        </div>
      </div>

      <Modal
        open={accountEditorOpen}
        onExitTransitionEnd={accountEditorAfterExit}
        title={accountEditor?.record ? t('sources.modal.edit_account') : t('sources.modal.new_account')}
        onClose={() => !mutationBusy && setAccountEditor(undefined)}
        closeDisabled={mutationBusy}
        width={680}
        footer={accountEditor && (
          <FormActions form="account-editor-form" cancelLabel={t('common.cancel')} submitLabel={accountEditor.record ? t('common.save_changes') : t('common.create')}
            busy={mutationBusy} onCancel={() => setAccountEditor(undefined)} />
        )}
      >
        {source && <AccountForm key={accountEditor?.record?.id ?? 'new-account'} record={accountEditor?.record} sources={[source]} busy={mutationBusy} error={formErrorMessage} onSubmit={submitAccount} />}
      </Modal>

      <ConfirmDialog
        open={deleteTargetOpen}
        onExitTransitionEnd={deleteTargetAfterExit}
        title={t('sources.confirm.delete_account_title')}
        description={<div className={styles.page}>{t('sources.confirm.delete_account_body', { id: deleteTarget?.id ?? '' })}<FormError message={formErrorMessage} /></div>}
        confirmLabel={t('common.delete')}
        danger
        busy={mutationBusy}
        onCancel={() => !mutationBusy && setDeleteTarget(undefined)}
        onConfirm={confirmDeleteAccount}
      />

      <ConfirmDialog
        open={sourceDeleteOpen}
        title={t('sources.confirm.delete_source_title')}
        description={<div className={styles.page}>{t('sources.confirm.delete_source_body', { id: sourceId ?? '' })}<FormError message={formErrorMessage} /></div>}
        confirmLabel={t('common.delete')}
        danger
        busy={mutationBusy}
        onCancel={() => !mutationBusy && setSourceDeleteOpen(false)}
        onConfirm={confirmDeleteSource}
      />
    </section>
  );
}
