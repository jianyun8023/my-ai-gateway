import { Table } from '@mantine/core';
import type {
  Account,
  AdminErrorShape,
  ConnectionTestResult,
  GatewayAdminResources,
  GatewayProtocol,
  Source
} from '@/admin-api';
import { normalizeAdminError } from '@/admin-api';
import { Button } from '@/components/ui/Button';
import { SelectField, TextField } from '@/components/ui/FormField';
import {
  IconCircleCheck,
  IconPencil,
  IconPlay,
  IconTriangleAlert
} from '@/components/ui/icons';
import { LoadingState } from '@/components/ui/LoadingState';
import { Modal } from '@/components/ui/Modal';
import { StatusPill } from '@/components/ui/StatusPill';
import { TableScroll } from '@/components/ui/TableScroll';
import styles from '@/features/control-plane/ControlPlane.module.scss';
import { DetailItem, DetailList, DrawerSection, EmptyTable, ErrorState, FormGrid, ProtocolPill } from '@/features/control-plane/shared';
import { protocolModeKey, protocolModeTone } from '@/features/control-plane/sources/presentation';
import { PROTOCOL_LABELS } from '@/lib/protocols';
import { useAdminQuery } from '@/hooks/useAdminQuery';
import { formatDateTime, formatJsonValue } from '@/utils/format';
import { useCallback, useState } from 'react';
import { useTranslation } from 'react-i18next';

export function SourceDetailDrawer({
  source,
  accounts,
  api,
  onClose,
  onEdit,
}: {
  source: Source;
  accounts: Account[];
  api: GatewayAdminResources;
  onClose: () => void;
  onEdit: () => void;
}) {
  const { t } = useTranslation('console');
  const [open, setOpen] = useState(true);
  const [editAfterExit, setEditAfterExit] = useState(false);
  const enabledAccounts = accounts.filter((account) => account.enabled);
  const [accountId, setAccountId] = useState(enabledAccounts[0]?.id ?? '');
  const [testModel, setTestModel] = useState('');
  const [testBusy, setTestBusy] = useState<GatewayProtocol>();
  const [testResults, setTestResults] = useState<Partial<Record<GatewayProtocol, ConnectionTestResult>>>({});
  const [testError, setTestError] = useState<AdminErrorShape>();


  const loadDiff = useCallback((signal: AbortSignal) => api.sourcePresetDiff(source.id, signal), [api, source.id]);
  const diffQuery = useAdminQuery({ load: loadDiff });
  const { data: diff, error: diffError } = diffQuery;
  const diffLoading = diffQuery.loading || diffQuery.refreshing;

  const runTest = async (protocol: GatewayProtocol) => {
    if (!accountId || testBusy) return;
    setTestBusy(protocol);
    setTestError(undefined);
    try {
      const result = await api.testConnection(source.id, {
        account_id: accountId,
        protocol,
        model: testModel.trim() || undefined,
        requested_by: 'admin-ui',
      });
      setTestResults((current) => ({ ...current, [protocol]: result }));
    } catch (error) {
      setTestError(normalizeAdminError(error));
    } finally {
      setTestBusy(undefined);
    }
  };

  return (
    <Modal
      open={open}
      variant="drawer"
      width={540}
      title={t('sources.detail.title_source')}
      onClose={() => setOpen(false)} onExitTransitionEnd={editAfterExit ? onEdit : onClose}
      footer={(
        <>
          <Button variant="secondary" onClick={() => setOpen(false)}>{t('common.close')}</Button>
          <Button variant="primary" onClick={() => { setEditAfterExit(true); setOpen(false); }}><IconPencil size={14} />{t('sources.modal.edit_source')}</Button>
        </>
      )}
    >
      <DrawerSection title={t('sources.detail.basic_info')}>
        <DetailList>
          <DetailItem label={t('sources.field.source')}><span className={styles.mono}>{source.id}</span></DetailItem>
          <DetailItem label={t('sources.field.display_name')}>{source.display_name}</DetailItem>
          <DetailItem label={t('sources.field.provider_preset')}><span className={styles.mono}>{source.provider_preset_id}@{source.provider_preset_version}</span></DetailItem>
          <DetailItem label={t('sources.field.base_url')}><span className={styles.mono}>{source.base_url}</span></DetailItem>
          <DetailItem label={t('common.status')}><StatusPill tone={source.enabled ? 'success' : 'muted'}>{source.enabled ? t('common.enabled') : t('common.disabled')}</StatusPill></DetailItem>
          <DetailItem label={t('common.updated_at')}>{formatDateTime(source.updated_at)}</DetailItem>
        </DetailList>
      </DrawerSection>

      <DrawerSection title={t('sources.detail.protocol_snapshot')}>
        <DetailList>
          {(['openai_chat_completions', 'openai_responses', 'anthropic_messages'] as const).map((protocol) => {
            const capability = source.protocol_capabilities[protocol];
            const endpointProtocol = capability?.source_protocol ?? protocol;
            return (
              <DetailItem key={protocol} label={PROTOCOL_LABELS[protocol]}>
                <span className={styles.inlineActions}>
                  <StatusPill tone={protocolModeTone(capability?.mode)}>{t(protocolModeKey(capability?.mode))}</StatusPill>
                  <span className={styles.mono}>{source.endpoints[endpointProtocol] ?? t('sources.detail.endpoint_unset')}{capability?.source_protocol && <small className={styles.blockMeta}>{t('sources.detail.upstream_endpoint')}</small>}</span>
                  {capability?.source_protocol && <span className={styles.secondaryText}>← {PROTOCOL_LABELS[capability.source_protocol]}</span>}
                </span>
              </DetailItem>
            );
          })}
        </DetailList>
      </DrawerSection>

      <DrawerSection title={t('sources.detail.preset_diff')}>
        {diffLoading ? <LoadingState label={t('sources.detail.preset_comparing')} /> : diffError ? <ErrorState error={diffError} onRetry={diffQuery.reload} /> : !diff || diff.changes.length === 0 ? (
          <EmptyTable title={t('sources.detail.preset_same')} />
        ) : (
          <div className={styles.page}>
            <div className={styles.inlineActions}>
              <StatusPill tone="accent">{t('sources.detail.preset_versions', { from: diff.source_version, to: diff.latest_version })}</StatusPill>
              <span className={styles.secondaryText}>{t('sources.detail.diff_count', { count: diff.changes.length })}</span>
            </div>
            <TableScroll label={t('sources.detail.preset_diff')}>
              <Table className={styles.table}>
                <Table.Thead><Table.Tr><Table.Th scope="col">{t('sources.detail.diff_path')}</Table.Th><Table.Th scope="col" miw={96}>{t('sources.detail.diff_type')}</Table.Th><Table.Th scope="col">{t('sources.detail.diff_old')}</Table.Th><Table.Th scope="col">{t('sources.detail.diff_new')}</Table.Th></Table.Tr></Table.Thead>
                <Table.Tbody>{diff.changes.map((change) => (
                  <Table.Tr key={`${change.kind}:${change.path}`}>
                    <Table.Td><code>{change.path}</code></Table.Td>
                    <Table.Td><StatusPill tone={change.kind === 'added' ? 'success' : change.kind === 'missing' ? 'danger' : 'warning'}>{change.kind}</StatusPill></Table.Td>
                    <Table.Td><code>{formatJsonValue(change.before)}</code></Table.Td>
                    <Table.Td><code>{formatJsonValue(change.after)}</code></Table.Td>
                  </Table.Tr>
                ))}</Table.Tbody>
              </Table>
            </TableScroll>
          </div>
        )}
      </DrawerSection>

      <DrawerSection title={t('sources.detail.connection_test')}>
        {enabledAccounts.length === 0 ? <EmptyTable title={t('sources.detail.test_no_account')} description={t('sources.detail.test_no_account_desc')} /> : (
          <div className={styles.page}>
            <FormGrid>
              <SelectField label={t('common.account')} value={accountId} disabled={Boolean(testBusy)} onChange={(event) => setAccountId(event.target.value)}>
                {enabledAccounts.map((account) => <option key={account.id} value={account.id}>{account.display_name} · {account.id}</option>)}
              </SelectField>
              <TextField label={t('sources.detail.test_model')} value={testModel} disabled={Boolean(testBusy)} onChange={(event) => setTestModel(event.target.value)} autoComplete="off" />
            </FormGrid>
            {testError && <ErrorState error={testError} />}
            <div className={styles.protocolTestGrid}>
              {(['openai_chat_completions', 'openai_responses', 'anthropic_messages'] as const).map((protocol) => {
                const result = testResults[protocol];
                const succeeded = result?.status === 'succeeded';
                return (
                  <div key={protocol} className={styles.protocolTestRow}>
                    <ProtocolPill protocol={protocol} />
                    {result && (
                      <span className={styles.primaryText}>
                        <strong>{succeeded ? <><IconCircleCheck size={14} /> {t('sources.detail.test_ok')}</> : <><IconTriangleAlert size={14} /> {result.status === 'failed' ? t('sources.detail.test_failed') : result.status}</>}</strong>
                        <small>{t(protocolModeKey(result.mode))} · {PROTOCOL_LABELS[result.upstream_protocol]} · {result.http_status ?? t('sources.detail.test_no_http')} · {result.latency_ms} ms</small>
                        {result.error_code && <small>{result.error_code}: {result.error_message}</small>}
                      </span>
                    )}
                    <Button size="sm" variant="secondary" loading={testBusy === protocol} disabled={Boolean(testBusy && testBusy !== protocol)} onClick={() => void runTest(protocol)}>
                      <IconPlay size={14} />{t('sources.detail.test_button')}
                    </Button>
                  </div>
                );
              })}
            </div>
          </div>
        )}
      </DrawerSection>
    </Modal>
  );
}
