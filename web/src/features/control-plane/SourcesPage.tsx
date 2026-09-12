import { clearOperationNotification, notifySuccess } from '@/components/ui/notifications';
import { Table } from '@mantine/core';
import type {
  Account,
  GatewayAdminResources,
  Source
} from '@/admin-api';
import { GATEWAY_PROTOCOLS } from '@/admin-api';
import { Button } from '@/components/ui/Button';
import { Card } from '@/components/ui/Card';
import { TextField } from '@/components/ui/FormField';
import { IconButton } from '@/components/ui/IconButton';
import {
  IconEye,
  IconPencil,
  IconPlay,
  IconPlus,
  IconRefreshCw,
} from '@/components/ui/icons';
import { LoadingState } from '@/components/ui/LoadingState';
import { MetricCard } from '@/components/ui/MetricCard';
import { StatusPill } from '@/components/ui/StatusPill';
import { TableScroll } from '@/components/ui/TableScroll';
import styles from '@/features/control-plane/ControlPlane.module.scss';
import { buildSyncStats, type SourceSyncStats } from '@/features/control-plane/discovery/model';
import {
  protocolModeKey,
  protocolModeTone,
  sourceConnection,
  sourceConnectionTone,
} from '@/features/control-plane/sources/presentation';
import { EmptyTable, ErrorState, Toggle } from '@/features/control-plane/shared';
import { useAdminQuery } from '@/hooks/useAdminQuery';
import { PROTOCOL_LABELS } from '@/lib/protocols';
import { sourceRouteHash, type SourceSection } from '@/lib/consoleNavigation';
import { formatDateTime } from '@/utils/format';
import { useCallback, useState } from 'react';
import { useTranslation } from 'react-i18next';

interface SourcesPageProps {
  api: GatewayAdminResources;
  refreshRevision?: number;
  onBusyChange?: (busy: boolean) => void;
  onOpenSource?: (sourceId: string, section?: SourceSection) => void;
}

interface SourcesData {
  sources: Source[];
  accounts: Account[];
  stats: Record<string, SourceSyncStats>;
}

export function SourcesPage({ api, refreshRevision = 0, onBusyChange, onOpenSource }: SourcesPageProps) {
  const { t } = useTranslation('console');
  const [search, setSearch] = useState('');
  const [checkingIds, setCheckingIds] = useState<ReadonlySet<string>>(() => new Set());
  const [actionError, setActionError] = useState<string>();

  const openSource = (sourceId: string, section?: SourceSection) => {
    if (onOpenSource) {
      onOpenSource(sourceId, section);
      return;
    }
    window.location.hash = sourceRouteHash(sourceId, section);
  };

  const load = useCallback(async (signal: AbortSignal): Promise<SourcesData> => {
    const [sources, accounts] = await Promise.all([
      api.sources(signal),
      api.accounts(signal),
    ]);
    const pairs = await Promise.all(sources.map(async (source) => {
      const [latest, models] = await Promise.all([
        api.latestDiscovery(source.id, signal),
        api.sourceModels(source.id, {}, signal),
      ]);
      return [source.id, buildSyncStats(latest, models)] as const;
    }));
    return { sources, accounts, stats: Object.fromEntries(pairs) };
  }, [api]);
  const query = useAdminQuery({ load, refreshRevision, onBusyChange });
  const data = query.data;

  const enabledAccount = (sourceId: string) => (
    data?.accounts.find((account) => account.source_id === sourceId && account.enabled)
  );

  const setChecking = (sourceId: string, checking: boolean) => {
    setCheckingIds((current) => {
      const next = new Set(current);
      if (checking) next.add(sourceId);
      else next.delete(sourceId);
      return next;
    });
  };

  const checkUpdates = async (source: Source) => {
    const account = enabledAccount(source.id);
    if (!account || checkingIds.has(source.id)) return;
    clearOperationNotification();
    setActionError(undefined);
    setChecking(source.id, true);
    onBusyChange?.(true);
    try {
      const execution = await api.runDiscovery(source.id, account.id);
      query.reload();
      // 运行失败/不支持由运行结果表达，不能提前宣告检查成功。
      if (execution.run.status === 'succeeded') {
        notifySuccess(t('sources.list.message.check_done', { name: source.display_name }));
        openSource(source.id, 'review');
        return;
      }
      setActionError(execution.run.status === 'unsupported'
        ? t('sources.list.message.check_unsupported', { name: source.display_name })
        : t('sources.list.message.check_failed', { name: source.display_name }));
    } catch {
      setActionError(t('sources.list.message.check_failed', { name: source.display_name }));
      query.reload();
    } finally {
      setChecking(source.id, false);
      onBusyChange?.(false);
    }
  };

  const batchCheck = async () => {
    if (!data || checkingIds.size > 0) return;
    clearOperationNotification();
    setActionError(undefined);
    const targets = data.sources.filter((source) => enabledAccount(source.id));
    if (targets.length === 0) return;
    setCheckingIds(new Set(targets.map((source) => source.id)));
    onBusyChange?.(true);
    let succeeded = 0;
    let unsupported = 0;
    await Promise.all(targets.map(async (source) => {
      const account = enabledAccount(source.id)!;
      try {
        const execution = await api.runDiscovery(source.id, account.id);
        if (execution.run.status === 'succeeded') succeeded += 1;
        else if (execution.run.status === 'unsupported') unsupported += 1;
        // failed 计入失败，不在这里重复计数成功。
      } catch {
        // 单个来源失败不影响其他来源，最终结果在汇总通知中体现。
      }
    }));
    setCheckingIds(new Set());
    onBusyChange?.(false);
    notifySuccess(t('sources.list.message.batch_check_done', {
      succeeded,
      failed: targets.length - succeeded - unsupported,
      unsupported,
    }));
    query.reload();
  };

  const toggleSource = async (source: Source) => {
    clearOperationNotification();
    setActionError(undefined);
    try {
      await api.setSourceEnabled(source.id, !source.enabled);
      notifySuccess(t(source.enabled ? 'sources.table.toggle_disabled' : 'sources.table.toggle_enabled', { name: source.id }));
      query.reload();
    } catch {
      setActionError(t('sources.list.message.toggle_failed', { name: source.id }));
      query.reload();
    }
  };

  if (query.loading && !data) return <LoadingState label={t('sources.loading')} />;
  if (query.error && !data) return <ErrorState error={query.error} onRetry={query.reload} />;
  if (!data) return null;

  const filteredSources = data.sources.filter((source) => {
    const term = search.trim().toLowerCase();
    if (!term) return true;
    return source.display_name.toLowerCase().includes(term) || source.id.toLowerCase().includes(term);
  });

  const enabledSources = data.sources.filter((source) => source.enabled).length;
  const healthyAccounts = data.accounts.filter((account) => account.health_status === 'healthy').length;
  const totalModels = Object.values(data.stats).reduce((sum, stats) => sum + stats.models.length, 0);
  const totalPending = Object.values(data.stats).reduce((sum, stats) => sum + stats.pendingCount, 0);
  const pendingSources = data.sources.filter((source) => (data.stats[source.id]?.pendingCount ?? 0) > 0).length;
  const batchTargets = data.sources.filter((source) => enabledAccount(source.id)).length;

  return (
    <section className={styles.page} data-od-id="page-sources">
      <p className={styles.secondaryText}>{t('sources.list.subtitle')}</p>

      <div className={styles.statsGrid}>
        <MetricCard label={t('sources.list.summary.sources')} value={String(data.sources.length)} hint={t('sources.list.summary.sources_hint', { count: enabledSources })} />
        <MetricCard label={t('sources.list.summary.accounts')} value={String(healthyAccounts)} hint={t('sources.list.summary.accounts_hint', { total: data.accounts.length })} tone={healthyAccounts < data.accounts.length ? 'warning' : 'success'} />
        <MetricCard label={t('sources.list.summary.models')} value={String(totalModels)} hint={t('sources.list.summary.models_hint')} />
        <MetricCard label={t('sources.list.summary.pending')} value={String(totalPending)} exact={String(totalPending)} hint={t('sources.list.summary.pending_hint', { count: pendingSources })} tone={totalPending > 0 ? 'warning' : 'success'} />
      </div>

      {query.error && <ErrorState error={query.error} onRetry={query.reload} />}
      {actionError && <ErrorState error={{ message: actionError }} />}

      {data.sources.length === 0 ? (
        <Card>
          <EmptyTable title={t('sources.empty.sources_title')} description={t('sources.empty.sources_desc')} />
          <div className={styles.cardActions}>
            <Button variant="primary" onClick={() => openSource('new', 'edit')}><IconPlus size={14} />{t('sources.add_source')}</Button>
          </div>
        </Card>
      ) : (
        <Card
          variant="flush"
          title={t('sources.list.card_title', { count: data.sources.length })}
          extra={(
            <div className={styles.rowActions}>
              <TextField
                label={t('sources.list.search')}
                value={search}
                onChange={(event) => setSearch(event.target.value)}
                autoComplete="off"
              />
              <Button variant="secondary" onClick={() => void batchCheck()} loading={checkingIds.size > 0} disabled={batchTargets === 0}>
                <IconRefreshCw size={14} />{t('sources.list.batch_check')}
              </Button>
              <Button variant="primary" onClick={() => openSource('new', 'edit')}><IconPlus size={14} />{t('sources.add_source')}</Button>
            </div>
          )}
        >
          {filteredSources.length === 0 ? <EmptyTable title={t('sources.list.search_empty')} /> : (
            <TableScroll label={t('sources.table.sources_region')}>
              <Table className={styles.table}>
                <Table.Thead><Table.Tr>
                  <Table.Th scope="col">{t('sources.field.source')}</Table.Th>
                  <Table.Th scope="col">{t('sources.list.col.account')}</Table.Th>
                  <Table.Th scope="col">{t('sources.list.col.connection')}</Table.Th>
                  <Table.Th scope="col">{t('sources.list.col.upstream')}</Table.Th>
                  <Table.Th scope="col">{t('sources.list.col.last_sync')}</Table.Th>
                  <Table.Th scope="col">{t('sources.list.col.pending')}</Table.Th>
                  <Table.Th scope="col">{t('sources.list.col.protocols')}</Table.Th>
                  <Table.Th scope="col">{t('common.status')}</Table.Th>
                  <Table.Th scope="col">{t('common.actions')}</Table.Th>
                </Table.Tr></Table.Thead>
                <Table.Tbody>{filteredSources.map((source) => {
                  const stats = data.stats[source.id];
                  const accounts = data.accounts.filter((account) => account.source_id === source.id);
                  const connection = sourceConnection(accounts);
                  const checking = checkingIds.has(source.id);
                  return (
                    <Table.Tr key={source.id} data-clickable="true" onClick={() => openSource(source.id)}>
                      <Table.Td><span className={styles.primaryText}><strong>{source.display_name}</strong><small className={styles.mono}>{source.id}</small></span></Table.Td>
                      <Table.Td>{accounts.length === 0 ? <span className={styles.secondaryText}>{t('sources.list.no_account')}</span> : (
                        <span className={styles.primaryText}><strong>{accounts[0].display_name}</strong><small>{t('sources.list.account_count', { count: accounts.length })}</small></span>
                      )}</Table.Td>
                      <Table.Td>
                        <span className={styles.inlineActions}>
                          <StatusPill tone={sourceConnectionTone(connection.state)}>{t(`sources.connection.${connection.state}`)}</StatusPill>
                          {connection.total > 0 && <small className={styles.secondaryText}>{t('sources.list.connection_ratio', { healthy: connection.healthy, total: connection.total })}</small>}
                        </span>
                      </Table.Td>
                      <Table.Td>{!stats || (stats.latest === null && stats.models.length === 0) ? <span className={styles.secondaryText}>{t('sources.list.never_synced')}</span> : (
                        <span className={styles.primaryText}>
                          <strong>{t('sources.list.model_count', { count: stats.models.length })}</strong>
                          <small>{t('sources.list.pending_count', { count: stats.pendingCount })}</small>
                        </span>
                      )}</Table.Td>
                      <Table.Td>{stats?.lastSyncAt ? formatDateTime(stats.lastSyncAt) : <span className={styles.secondaryText}>—</span>}</Table.Td>
                      <Table.Td><StatusPill tone={stats && stats.pendingCount > 0 ? 'warning' : 'muted'}>{stats?.pendingCount ?? 0}</StatusPill></Table.Td>
                      <Table.Td><span className={styles.inlineActions}>
                        {GATEWAY_PROTOCOLS.map((protocol) => {
                          const label = t(protocolModeKey(source.protocol_capabilities[protocol]?.mode));
                          return <StatusPill key={protocol} tone={protocolModeTone(source.protocol_capabilities[protocol]?.mode)} title={`${PROTOCOL_LABELS[protocol]} · ${label}`}>{label}</StatusPill>;
                        })}
                      </span></Table.Td>
                      <Table.Td onClick={(event) => event.stopPropagation()}>
                        <Toggle label={t('sources.table.toggle_aria', { id: source.id })} checked={source.enabled} disabled={checking} onChange={() => void toggleSource(source)} />
                      </Table.Td>
                      <Table.Td onClick={(event) => event.stopPropagation()}><div className={styles.rowActions}>
                        <IconButton label={t('sources.table.view_aria', { id: source.id })} onClick={() => openSource(source.id)}><IconEye size={16} /></IconButton>
                        <IconButton label={t('sources.table.edit_aria', { id: source.id })} onClick={() => openSource(source.id, 'edit')}><IconPencil size={16} /></IconButton>
                        <IconButton
                          label={t('sources.list.check_updates_aria', { id: source.id })}
                          disabled={!enabledAccount(source.id) || checkingIds.size > 0}
                          loading={checking}
                          onClick={() => void checkUpdates(source)}
                        ><IconPlay size={16} /></IconButton>
                      </div></Table.Td>
                    </Table.Tr>
                  );
                })}</Table.Tbody>
              </Table>
            </TableScroll>
          )}
        </Card>
      )}

      <Card className={styles.flowCard}>
        <strong>{t('sources.list.flow_title')}</strong>
        <span className={styles.secondaryText}>{t('sources.list.flow_steps')}</span>
      </Card>
    </section>
  );
}
