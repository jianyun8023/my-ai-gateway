import { DetailItem, DetailList } from '@/components/ui/DetailList';
import { Table } from '@mantine/core';
import type {
  Account,
  AdminErrorShape,
  ConnectionTestResult,
  GatewayAdminResources,
  GatewayProtocol,
  RuntimeEventRecord,
  Source,
} from '@/admin-api';
import { GATEWAY_PROTOCOLS, normalizeAdminError } from '@/admin-api';
import { Button } from '@/components/ui/Button';
import { Card } from '@/components/ui/Card';
import { SelectField, TextField } from '@/components/ui/FormField';
import {
  IconCircleCheck,
  IconPencil,
  IconPlay,
  IconTriangleAlert,
} from '@/components/ui/icons';
import { LoadingState } from '@/components/ui/LoadingState';
import { StatusPill } from '@/components/ui/StatusPill';
import { TableScroll } from '@/components/ui/TableScroll';
import { clearOperationNotification, notifySuccess } from '@/components/ui/notifications';
import styles from '@/features/control-plane/ControlPlane.module.scss';
import {
  buildSyncStats,
  capabilitySummary,
  type SourceSyncStats,
} from '@/features/control-plane/discovery/model';
import {
  credentialKey,
  protocolModeKey,
  protocolModeTone,
  sourceConnection,
} from '@/features/control-plane/sources/presentation';
import { EmptyTable, ErrorState, FormGrid, ProtocolPill } from '@/features/control-plane/shared';
import { useAdminQuery } from '@/hooks/useAdminQuery';
import { PROTOCOL_LABELS } from '@/lib/protocols';
import { sourceRouteHash, type SourceSection } from '@/lib/consoleNavigation';
import { formatDateTime, formatJsonValue } from '@/utils/format';
import { useCallback, useState } from 'react';
import { useTranslation } from 'react-i18next';

interface SourceDetailPageProps {
  api: GatewayAdminResources;
  refreshRevision?: number;
  onBusyChange?: (busy: boolean) => void;
  sourceId: string;
  onOpenSource?: (sourceId: string, section?: SourceSection) => void;
}

interface SourceContext {
  source?: Source;
  accounts: Account[];
}

interface SourceDetailView {
  stats: SourceSyncStats;
  events: RuntimeEventRecord[];
}

export function SourceDetailPage({ api, refreshRevision = 0, onBusyChange, sourceId, onOpenSource }: SourceDetailPageProps) {
  const { t } = useTranslation('console');
  const [testModel, setTestModel] = useState('');
  const [testBusy, setTestBusy] = useState<GatewayProtocol>();
  const [testingAll, setTestingAll] = useState(false);
  const [testResults, setTestResults] = useState<Partial<Record<GatewayProtocol, ConnectionTestResult>>>({});
  const [testError, setTestError] = useState<AdminErrorShape>();
  const [actionError, setActionError] = useState<string>();

  const openSource = (targetId: string, section?: SourceSection) => {
    if (onOpenSource) {
      onOpenSource(targetId, section);
      return;
    }
    window.location.hash = sourceRouteHash(targetId, section);
  };

  const loadContext = useCallback(async (signal: AbortSignal): Promise<SourceContext> => {
    const [sources, accounts] = await Promise.all([api.sources(signal), api.accounts(signal)]);
    return { source: sources.find((item) => item.id === sourceId), accounts };
  }, [api, sourceId]);
  const contextQuery = useAdminQuery({ load: loadContext, refreshRevision, onBusyChange });
  const context = contextQuery.data;
  const source = context?.source;
  const accounts = (context?.accounts ?? []).filter((account) => account.source_id === sourceId);
  const enabledAccounts = accounts.filter((account) => account.enabled);
  const [accountId, setAccountId] = useState('');
  const effectiveAccountId = enabledAccounts.some((account) => account.id === accountId)
    ? accountId
    : enabledAccounts[0]?.id ?? '';

  const loadDetail = useCallback(async (signal: AbortSignal): Promise<SourceDetailView> => {
    const [latest, models, eventResponse] = await Promise.all([
      api.latestDiscovery(sourceId, signal),
      api.sourceModels(sourceId, {}, signal),
      api.runtimeEvents({ subject_type: 'source', subject_id: sourceId, limit: 6 }, signal),
    ]);
    return { stats: buildSyncStats(latest, models), events: eventResponse.data };
  }, [api, sourceId]);
  const detailQuery = useAdminQuery({ load: loadDetail, queryKey: sourceId, refreshRevision });
  const detail = detailQuery.data;
  const stats = detail?.stats;

  const runTest = async (protocol: GatewayProtocol) => {
    if (!effectiveAccountId || testBusy || testingAll) return;
    setTestBusy(protocol);
    setTestError(undefined);
    try {
      const result = await api.testConnection(sourceId, {
        account_id: effectiveAccountId,
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

  const testAll = async () => {
    if (!effectiveAccountId || testBusy || testingAll) return;
    setTestingAll(true);
    setTestError(undefined);
    try {
      const results = await Promise.all(GATEWAY_PROTOCOLS.map(async (protocol) => {
        try {
          return [protocol, await api.testConnection(sourceId, {
            account_id: effectiveAccountId,
            protocol,
            model: testModel.trim() || undefined,
            requested_by: 'admin-ui',
          })] as const;
        } catch {
          return [protocol, undefined] as const;
        }
      }));
      setTestResults((current) => ({ ...current, ...Object.fromEntries(results.filter(([, value]) => value)) }));
    } finally {
      setTestingAll(false);
    }
  };

  const checkUpdates = async () => {
    if (!effectiveAccountId || !stats || detailQuery.refreshing) return;
    clearOperationNotification();
    setActionError(undefined);
    onBusyChange?.(true);
    try {
      await api.runDiscovery(sourceId, effectiveAccountId);
      notifySuccess(t('sources.list.message.check_done', { name: source?.display_name ?? sourceId }));
      detailQuery.reload();
      openSource(sourceId, 'review');
    } catch {
      setActionError(t('sources.list.message.check_failed', { name: source?.display_name ?? sourceId }));
      detailQuery.reload();
    } finally {
      onBusyChange?.(false);
    }
  };

  if (contextQuery.loading && !context) return <LoadingState label={t('sources.loading')} />;
  if (contextQuery.error && !context) return <ErrorState error={contextQuery.error} onRetry={contextQuery.reload} />;
  if (context && !source) {
    return (
      <section className={styles.page} data-od-id="page-source-detail">
        <EmptyTable title={t('sources.detail.missing_title', { id: sourceId })} description={t('sources.detail.missing_desc')} />
        {contextQuery.error && <ErrorState error={contextQuery.error} onRetry={contextQuery.reload} />}
        <div className={styles.cardActions}>
          <Button variant="secondary" onClick={() => openSource('')}>{t('sources.detail.back_to_list')}</Button>
        </div>
      </section>
    );
  }
  if (!source) return null;

  const connection = sourceConnection(accounts);

  return (
    <section className={styles.page} data-od-id="page-source-detail">
      <div className={styles.breadcrumbBar}>
        <Button size="sm" variant="ghost" onClick={() => openSource('')}>{t('sources.detail.back_to_list')}</Button>
        <span aria-hidden="true">/</span>
        <strong>{source.display_name}</strong>
      </div>

      <Card>
        <div className={styles.detailHeader}>
          <h2>
            {source.display_name}
            <StatusPill tone={source.enabled ? 'success' : 'muted'}>{source.enabled ? t('common.enabled') : t('common.disabled')}</StatusPill>
          </h2>
          <div className={styles.metaStrip}>
            <span>{t('sources.detail.meta.connection')}<strong>{t(`sources.connection.${connection.state}`)}</strong></span>
            <span>{t('sources.detail.meta.accounts')}<strong>{accounts.filter((account) => account.health_status === 'healthy').length}/{accounts.length}</strong></span>
            <span>{t('sources.detail.meta.models')}<strong>{stats?.models.length ?? '—'}</strong></span>
            <span>{t('sources.detail.meta.pending')}<strong>{stats?.pendingCount ?? '—'}</strong></span>
          </div>
          <div className={styles.rowActions}>
            <Button variant="secondary" onClick={() => openSource(source.id, 'edit')}><IconPencil size={14} />{t('sources.modal.edit_source')}</Button>
            <Button variant="secondary" loading={testingAll} disabled={!effectiveAccountId || Boolean(testBusy)} onClick={() => void testAll()}>{t('sources.detail.test_connection')}</Button>
            <Button variant="secondary" loading={detailQuery.refreshing} disabled={!effectiveAccountId} onClick={() => void checkUpdates()}>{t('sources.detail.check_updates')}</Button>
            {(stats?.pendingCount ?? 0) > 0 && (
              <Button variant="primary" onClick={() => openSource(source.id, 'review')}>{t('sources.detail.review_changes')}</Button>
            )}
          </div>
        </div>
      </Card>

      {contextQuery.error && <ErrorState error={contextQuery.error} onRetry={contextQuery.reload} />}
      {detailQuery.error && <ErrorState error={detailQuery.error} onRetry={detailQuery.reload} />}
      {actionError && <ErrorState error={{ message: actionError }} />}

      <div className={styles.detailGrid}>
        <Card title={t('sources.detail.basic_info')}>
          <DetailList>
            <DetailItem label={t('sources.field.source_id')}><span className={styles.mono}>{source.id}</span></DetailItem>
            <DetailItem label={t('sources.field.provider_preset')}><span className={styles.mono}>{source.provider_preset_id}@{source.provider_preset_version}</span></DetailItem>
            <DetailItem label={t('sources.field.base_url')}><span className={styles.mono}>{source.base_url}</span></DetailItem>
            <DetailItem label={t('common.updated_at')}>{formatDateTime(source.updated_at)}</DetailItem>
          </DetailList>
        </Card>

        <Card
          title={t('sources.detail.accounts_card')}
          extra={<Button size="sm" variant="ghost" onClick={() => openSource(source.id, 'edit')}>{t('sources.detail.manage_accounts')}</Button>}
        >
          {accounts.length === 0 ? <EmptyTable title={t('sources.detail.no_accounts')} description={t('sources.detail.no_accounts_desc')} /> : accounts.map((account) => (
            <div key={account.id} className={styles.accountRow}>
              <span className={styles.primaryText}>
                <strong>{account.display_name}</strong>
                <small className={styles.mono}>{account.id}</small>
              </span>
              <StatusPill tone={account.health_status === 'healthy' ? 'success' : account.health_status === 'unknown' ? 'accent' : 'warning'}>{t(`values.health.${account.health_status || 'unknown'}`, { defaultValue: account.health_status || 'unknown' })}</StatusPill>
              <StatusPill tone={account.credential_configured ? 'success' : 'muted'}>{t(credentialKey(account))}</StatusPill>
              <small className={styles.secondaryText}>{account.last_probe_at ? t('sources.detail.last_probe', { time: formatDateTime(account.last_probe_at) }) : t('sources.detail.never_probed')}</small>
            </div>
          ))}
        </Card>

        <Card title={t('sources.detail.protocol_card')}>
          <DetailList>
            {GATEWAY_PROTOCOLS.map((protocol) => {
              const capability = source.protocol_capabilities[protocol];
              const endpointProtocol = capability?.source_protocol ?? protocol;
              return (
                <DetailItem key={protocol} label={PROTOCOL_LABELS[protocol]}>
                  <span className={styles.inlineActions}>
                    <span className={styles.mono}>{source.endpoints[endpointProtocol] ?? t('sources.detail.endpoint_unset')}</span>
                    <StatusPill tone={protocolModeTone(capability?.mode)}>{t(protocolModeKey(capability?.mode))}</StatusPill>
                    {capability?.source_protocol && <small className={styles.secondaryText}>← {PROTOCOL_LABELS[capability.source_protocol]}</small>}
                  </span>
                </DetailItem>
              );
            })}
          </DetailList>
        </Card>

        <Card
          title={t('sources.detail.sync_card')}
          extra={(
            <div className={styles.rowActions}>
              <Button size="sm" variant="secondary" loading={detailQuery.refreshing} disabled={!effectiveAccountId} onClick={() => void checkUpdates()}>{t('sources.detail.check_updates')}</Button>
              {(stats?.pendingCount ?? 0) > 0 && <Button size="sm" variant="primary" onClick={() => openSource(source.id, 'review')}>{t('sources.detail.review_changes')}</Button>}
            </div>
          )}
        >
          {detailQuery.loading && !detail ? <LoadingState label={t('discovery.loading_run')} /> : detailQuery.error && !detail ? <ErrorState error={detailQuery.error} onRetry={detailQuery.reload} /> : !stats || (stats.latest === null && stats.models.length === 0) ? (
            <EmptyTable title={t('sources.detail.never_synced')} description={t('sources.detail.never_synced_desc')} />
          ) : (
            <div className={styles.stack}>
              <span className={styles.secondaryText}>
                {stats.lastSyncAt
                  ? t('sources.detail.last_sync', { time: formatDateTime(stats.lastSyncAt), duration: stats.latest?.run.latency_ms })
                  : t('sources.detail.never_synced')}
              </span>
              {stats.latest && <StatusPill tone={stats.latest.run.status === 'succeeded' ? 'success' : stats.latest.run.status === 'failed' ? 'danger' : 'warning'}>{t(`discovery.run_state.${stats.latest.run.status}`, { defaultValue: stats.latest.run.status })}</StatusPill>}
              <div className={styles.inlineActions}>
                <StatusPill tone="success">{t('discovery.diff_column.added')} {stats.addedCount}</StatusPill>
                <StatusPill tone="warning">{t('discovery.diff_column.changed')} {stats.changedCount}</StatusPill>
                <StatusPill tone={stats.missingCount > 0 ? 'danger' : 'muted'}>{t('discovery.diff_column.missing')} {stats.missingCount}</StatusPill>
                <StatusPill tone={stats.pendingCount > 0 ? 'warning' : 'muted'}>{t('sources.list.col.pending')} {stats.pendingCount}</StatusPill>
              </div>
            </div>
          )}
        </Card>

        <Card
          data-span="full"
          title={t('sources.detail.models_card')}
          extra={<Button size="sm" variant="ghost" onClick={() => openSource(source.id, 'review')}>{t('sources.detail.view_all_models')}</Button>}
        >
          {detailQuery.loading && !detail ? <LoadingState label={t('discovery.loading_models')} /> : detailQuery.error && !detail ? null : !stats || stats.models.length === 0 ? (
            <EmptyTable title={t('sources.detail.no_models')} description={t('sources.detail.no_models_desc')} />
          ) : (
            <TableScroll label={t('sources.detail.models_region')}>
              <Table className={styles.table}>
                <Table.Thead><Table.Tr>
                  <Table.Th scope="col">{t('discovery.column.upstream_model')}</Table.Th>
                  <Table.Th scope="col">{t('sources.detail.suggested_logical')}</Table.Th>
                  <Table.Th scope="col">{t('sources.detail.capabilities')}</Table.Th>
                  <Table.Th scope="col">{t('discovery.column.confirmation')}</Table.Th>
                  <Table.Th scope="col">{t('discovery.column.availability')}</Table.Th>
                </Table.Tr></Table.Thead>
                <Table.Tbody>{stats.models.slice(0, 8).map((model) => (
                  <Table.Tr key={model.upstream_model_id}>
                    <Table.Td><code>{model.upstream_model_id}</code></Table.Td>
                    <Table.Td>{typeof model.metadata.logical_model_name === 'string' && model.metadata.logical_model_name ? model.metadata.logical_model_name : <span className={styles.secondaryText}>—</span>}</Table.Td>
                    <Table.Td><span className={styles.secondaryText}>{capabilitySummary(model, t) || '—'}</span></Table.Td>
                    <Table.Td><StatusPill tone={model.confirmation_status === 'confirmed' ? 'success' : 'warning'}>{t(`discovery.confirm_state.${model.confirmation_status}`)}</StatusPill></Table.Td>
                    <Table.Td><StatusPill tone={model.availability_status === 'available' ? 'success' : model.availability_status === 'unavailable' ? 'danger' : 'accent'}>{t(`discovery.availability_state.${model.availability_status}`)}</StatusPill></Table.Td>
                  </Table.Tr>
                ))}</Table.Tbody>
              </Table>
            </TableScroll>
          )}
        </Card>

        <Card title={t('sources.detail.connection_test')}>
          {enabledAccounts.length === 0 ? <EmptyTable title={t('sources.detail.test_no_account')} description={t('sources.detail.test_no_account_desc')} /> : (
            <div className={styles.stack}>
              <FormGrid>
                <SelectField
                  label={t('common.account')}
                  value={effectiveAccountId}
                  disabled={Boolean(testBusy) || testingAll}
                  data={enabledAccounts.map((account) => ({ value: account.id, label: `${account.display_name} · ${account.id}` }))}
                  onChange={setAccountId}
                />
                <TextField label={t('sources.detail.test_model')} value={testModel} disabled={Boolean(testBusy) || testingAll} onChange={(event) => setTestModel(event.target.value)} autoComplete="off" />
              </FormGrid>
              {testError && <ErrorState error={testError} />}
              <div className={styles.protocolTestGrid}>
                {GATEWAY_PROTOCOLS.map((protocol) => {
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
                      <Button size="sm" variant="secondary" loading={testBusy === protocol} disabled={Boolean(testBusy && testBusy !== protocol) || testingAll} onClick={() => void runTest(protocol)}>
                        <IconPlay size={14} />{t('sources.detail.test_button')}
                      </Button>
                    </div>
                  );
                })}
              </div>
            </div>
          )}
        </Card>

        <Card title={t('sources.detail.events_card')}>
          {detailQuery.loading && !detail ? <LoadingState label={t('sources.detail.events_loading')} /> : !detail || detail.events.length === 0 ? (
            <EmptyTable title={t('sources.detail.events_empty')} description={t('sources.detail.events_empty_desc')} />
          ) : (
            <ul className={styles.eventList}>
              {detail.events.map((event) => (
                <li key={event.event_id}>
                  <strong>{event.event_type}</strong>
                  <small>{event.message}</small>
                  <small>{formatDateTime(event.occurred_at)}</small>
                </li>
              ))}
            </ul>
          )}
        </Card>
      </div>

      <PresetDiffCard api={api} source={source} />
    </section>
  );
}

function PresetDiffCard({ api, source }: { api: GatewayAdminResources; source: Source }) {
  const { t } = useTranslation('console');
  const loadDiff = useCallback((signal: AbortSignal) => api.sourcePresetDiff(source.id, signal), [api, source.id]);
  const diffQuery = useAdminQuery({ load: loadDiff });
  const { data: diff, error: diffError } = diffQuery;
  return (
    <Card title={t('sources.detail.preset_diff')}>
      {diffQuery.loading || diffQuery.refreshing ? <LoadingState label={t('sources.detail.preset_comparing')} /> : diffError ? <ErrorState error={diffError} onRetry={diffQuery.reload} /> : !diff || diff.changes.length === 0 ? (
        <EmptyTable title={t('sources.detail.preset_same')} />
      ) : (
        <div className={styles.stack}>
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
    </Card>
  );
}
