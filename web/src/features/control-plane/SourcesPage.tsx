import { clearOperationNotification, notifySuccess } from '@/components/ui/notifications';
import { Table } from '@mantine/core';
import type {
  Account,
  AdminErrorShape,
  GatewayAdminResources,
  Source
} from '@/admin-api';
import { GATEWAY_PROTOCOLS, normalizeAdminError } from '@/admin-api';
import { Button } from '@/components/ui/Button';
import { Card } from '@/components/ui/Card';
import { TextField } from '@/components/ui/FormField';
import { IconButton } from '@/components/ui/IconButton';
import {
  IconEye,
  IconPencil,
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
  sourceConnection,
  sourceConnectionTone,
} from '@/features/control-plane/sources/presentation';
import { EmptyTable, ErrorState, Toggle } from '@/features/control-plane/shared';
import { useAdminQuery } from '@/hooks/useAdminQuery';
import { PROTOCOL_LABELS, PROTOCOL_SHORT_LABELS } from '@/lib/protocols';
import sourceStyles from './sources/SourcesPage.module.scss';
import { sourceRouteHash, type SourceSection } from '@/lib/consoleNavigation';
import { formatDateTime } from '@/utils/format';
import { useCallback, useEffect, useRef, useState } from 'react';
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
}

type SourceStatsState =
  | { status: 'loading' }
  | { status: 'ready'; data: SourceSyncStats }
  | { status: 'error'; error: AdminErrorShape };

export function SourcesPage({ api, refreshRevision = 0, onBusyChange, onOpenSource }: SourcesPageProps) {
  const { t } = useTranslation('console');
  const [search, setSearch] = useState('');
  const [checkingIds, setCheckingIds] = useState<ReadonlySet<string>>(() => new Set());
  const [actionError, setActionError] = useState<string>();
  const [statsBySource, setStatsBySource] = useState<Record<string, SourceStatsState>>({});
  const statsControllers = useRef(new Map<string, AbortController>());

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
    return { sources, accounts };
  }, [api]);
  const query = useAdminQuery({ load, refreshRevision, onBusyChange });
  const data = query.data;

  const loadSourceStats = useCallback((sourceId: string) => {
    statsControllers.current.get(sourceId)?.abort();
    const controller = new AbortController();
    statsControllers.current.set(sourceId, controller);
    setStatsBySource((current) => ({ ...current, [sourceId]: { status: 'loading' } }));
    void Promise.all([
      api.latestDiscovery(sourceId, controller.signal),
      api.sourceModels(sourceId, {}, controller.signal),
    ]).then(([latest, models]) => {
      if (controller.signal.aborted || statsControllers.current.get(sourceId) !== controller) return;
      setStatsBySource((current) => ({ ...current, [sourceId]: { status: 'ready', data: buildSyncStats(latest, models) } }));
    }).catch((error) => {
      if (controller.signal.aborted || statsControllers.current.get(sourceId) !== controller) return;
      setStatsBySource((current) => ({ ...current, [sourceId]: { status: 'error', error: normalizeAdminError(error) } }));
    }).finally(() => {
      if (statsControllers.current.get(sourceId) === controller) statsControllers.current.delete(sourceId);
    });
  }, [api]);

  useEffect(() => {
    const controllers = statsControllers.current;
    const sourceIds = new Set(data?.sources.map((source) => source.id) ?? []);
    setStatsBySource((current) => Object.fromEntries(Object.entries(current).filter(([sourceId]) => sourceIds.has(sourceId))));
    for (const sourceId of sourceIds) loadSourceStats(sourceId);
    return () => {
      for (const controller of controllers.values()) controller.abort();
      controllers.clear();
    };
  }, [data?.sources, loadSourceStats]);

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
  const readyStats = data.sources.flatMap((source) => {
    const state = statsBySource[source.id];
    return state?.status === 'ready' ? [state.data] : [];
  });
  const statsComplete = readyStats.length === data.sources.length;
  const statsFailed = data.sources.filter((source) => statsBySource[source.id]?.status === 'error').length;
  const totalModels = readyStats.reduce((sum, stats) => sum + stats.models.length, 0);
  const totalPending = readyStats.reduce((sum, stats) => sum + stats.pendingCount, 0);
  const pendingSources = readyStats.filter((stats) => stats.pendingCount > 0).length;
  const statsHint = statsFailed > 0
    ? t('sources.list.summary.stats_failed', { failed: statsFailed, total: data.sources.length })
    : t('sources.list.summary.stats_loading', { ready: readyStats.length, total: data.sources.length });
  const batchTargets = data.sources.filter((source) => enabledAccount(source.id)).length;

  return (
    <section className={styles.page} data-od-id="page-sources">
      <div className={`${styles.statsGrid} ${sourceStyles.summary}`}>
        <MetricCard compact label={t('sources.list.summary.sources')} value={String(data.sources.length)} hint={t('sources.list.summary.sources_hint', { count: enabledSources })} />
        <MetricCard compact label={t('sources.list.summary.accounts')} value={String(healthyAccounts)} hint={t('sources.list.summary.accounts_hint', { total: data.accounts.length })} tone={healthyAccounts < data.accounts.length ? 'warning' : undefined} />
        <MetricCard compact label={t('sources.list.summary.models')} value={statsComplete ? String(totalModels) : '—'} hint={statsComplete ? t('sources.list.summary.models_hint') : statsHint} />
        <MetricCard compact label={t('sources.list.summary.pending')} value={statsComplete ? String(totalPending) : '—'} exact={statsComplete ? String(totalPending) : undefined} hint={statsComplete ? t('sources.list.summary.pending_hint', { count: pendingSources }) : statsHint} tone={statsComplete && totalPending > 0 ? 'warning' : undefined} />
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
          className={sourceStyles.listCard}
          title={t('sources.list.card_title', { count: data.sources.length })}
          extra={(
            <div className={sourceStyles.toolbar}>
              <TextField
                className={sourceStyles.search}
                label={t('sources.list.search')}
                placeholder={t('sources.list.search')}
                type="search"
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
              <Table className={`${styles.table} ${sourceStyles.table}`}>
                <Table.Thead><Table.Tr>
                  <Table.Th scope="col">{t('sources.field.source')}</Table.Th>
                  <Table.Th scope="col">{t('sources.list.col.account')}</Table.Th>
                  <Table.Th scope="col">{t('sources.list.col.connection')}</Table.Th>
                  <Table.Th scope="col">{t('sources.list.col.upstream')}</Table.Th>
                  <Table.Th scope="col">{t('sources.list.col.last_sync')}</Table.Th>
                  <Table.Th scope="col">{t('sources.list.col.protocols')}</Table.Th>
                  <Table.Th scope="col">{t('common.status')}</Table.Th>
                  <Table.Th scope="col">{t('common.actions')}</Table.Th>
                </Table.Tr></Table.Thead>
                <Table.Tbody>{filteredSources.map((source) => {
                  const statsState = statsBySource[source.id];
                  const stats = statsState?.status === 'ready' ? statsState.data : undefined;
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
                      <Table.Td>{!statsState || statsState.status === 'loading' ? <LoadingState layout="inline" label={t('sources.list.stats_loading')} /> : statsState.status === 'error' ? (
                        <span className={styles.primaryText}>
                          <strong>{t('sources.list.stats_failed')}</strong>
                          {statsState.error.code && <small><code>{statsState.error.code}</code></small>}
                          <Button size="sm" variant="ghost" onClick={(event) => { event.stopPropagation(); loadSourceStats(source.id); }}>{t('common.retry')}</Button>
                        </span>
                      ) : !stats ? null : (stats.latest === null && stats.models.length === 0) ? <span className={styles.secondaryText}>{t('sources.list.never_synced')}</span> : (
                        <span className={styles.primaryText}>
                          <strong>{t('sources.list.model_count', { count: stats.models.length })}</strong>
                          {stats.pendingCount > 0 && <StatusPill tone="warning">{t('sources.list.pending_count', { count: stats.pendingCount })}</StatusPill>}
                        </span>
                      )}</Table.Td>
                      <Table.Td>{stats?.lastSyncAt ? (
                        <time className={sourceStyles.syncTime} dateTime={stats.lastSyncAt} title={formatDateTime(stats.lastSyncAt)} aria-label={formatDateTime(stats.lastSyncAt)}>
                          <span>{formatDateTime(stats.lastSyncAt, { dateStyle: 'short' })}</span>
                          <small>{formatDateTime(stats.lastSyncAt, { timeStyle: 'short' })}</small>
                        </time>
                      ) : statsState?.status === 'ready' ? <span className={styles.secondaryText}>—</span> : <span className={styles.secondaryText}>{t('sources.list.stats_unknown')}</span>}</Table.Td>
                      <Table.Td><span className={sourceStyles.protocols}>
                        {GATEWAY_PROTOCOLS.map((protocol) => {
                          const mode = source.protocol_capabilities[protocol]?.mode;
                          const label = t(protocolModeKey(mode));
                          return <span key={protocol} className={sourceStyles.protocol} title={`${PROTOCOL_LABELS[protocol]} · ${label}`}>
                            <span>{PROTOCOL_SHORT_LABELS[protocol]}</span>
                            <StatusPill tone={mode === 'adapter' ? 'warning' : 'muted'}>{label}</StatusPill>
                          </span>;
                        })}
                      </span></Table.Td>
                      <Table.Td onClick={(event) => event.stopPropagation()}>
                        <Toggle label={t('sources.table.toggle_aria', { id: source.id })} checked={source.enabled} disabled={checking} onChange={() => void toggleSource(source)} />
                      </Table.Td>
                      <Table.Td onClick={(event) => event.stopPropagation()}><div className={styles.rowActions}>
                        <IconButton label={t('sources.table.view_aria', { id: source.id })} onClick={() => openSource(source.id)}><IconEye size={16} /></IconButton>
                        <IconButton label={t('sources.table.edit_aria', { id: source.id })} onClick={() => openSource(source.id, 'edit')}><IconPencil size={16} /></IconButton>
                        <Button variant="ghost" size="sm"
                          aria-label={t('sources.list.check_updates_aria', { id: source.id })}
                          disabled={!enabledAccount(source.id) || checkingIds.size > 0}
                          loading={checking}
                          onClick={() => void checkUpdates(source)}
                        ><IconRefreshCw size={14} />{t('sources.list.check_updates')}</Button>
                      </div></Table.Td>
                    </Table.Tr>
                  );
                })}</Table.Tbody>
              </Table>
            </TableScroll>
          )}
        </Card>
      )}

      <aside className={sourceStyles.flow} aria-label={t('sources.list.flow_title')}>
        <strong>{t('sources.list.flow_title')}</strong>
        <p>{t('sources.list.flow_steps')}</p>
      </aside>
    </section>
  );
}
