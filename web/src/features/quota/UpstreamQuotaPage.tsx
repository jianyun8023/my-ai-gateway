import { Select, Table } from '@mantine/core';
import { Button } from '@/components/ui/Button';
import { Card } from '@/components/ui/Card';
import { CheckboxField } from '@/components/ui/CheckboxField';
import { DetailItem, DetailList } from '@/components/ui/DetailList';
import { EmptyState } from '@/components/ui/EmptyState';
import { TextField } from '@/components/ui/FormField';
import { IconRefreshCw } from '@/components/ui/icons';
import { LoadingState } from '@/components/ui/LoadingState';
import { MetricCard } from '@/components/ui/MetricCard';
import { Notice } from '@/components/ui/Notice';
import { StatusPill } from '@/components/ui/StatusPill';
import { TableScroll } from '@/components/ui/TableScroll';
import { formatDateTime } from '@/utils/format';
import { useCallback, useEffect, useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { UpstreamQuotaClient } from '@/upstream-quota/client';
import type {
  QuotaResource,
  UpstreamQuotaSnapshot,
  UpstreamQuotaStatus,
} from '@/upstream-quota/types';
import styles from './UpstreamQuota.module.scss';

interface UpstreamQuotaPageProps {
  client: UpstreamQuotaClient;
  accountId?: string;
  refreshRevision?: number;
  onBusyChange?: (busy: boolean) => void;
}

type QuotaTranslation = ReturnType<typeof useTranslation<'console'>>['t'];

const FAILED_STATUSES = new Set<UpstreamQuotaStatus>(['refresh_failed', 'auth_error']);
const PROBLEM_STATUSES = new Set<UpstreamQuotaStatus>([
  'low',
  'exhausted',
  'refresh_failed',
  'auth_error',
  'disabled',
]);

const providerLabel = (provider: string): string => ({
  kimi_code: 'Kimi Code',
  minimax: 'MiniMax',
  deepseek: 'DeepSeek',
}[provider] ?? provider);

const mergeSnapshot = (
  previous: UpstreamQuotaSnapshot | undefined,
  next: UpstreamQuotaSnapshot,
): UpstreamQuotaSnapshot => {
  if (
    previous
    && previous.resources.length > 0
    && next.resources.length === 0
    && FAILED_STATUSES.has(next.status)
  ) {
    return {
      ...next,
      resources: previous.resources,
      fetched_at: previous.fetched_at,
      raw: previous.raw,
      stale: true,
    };
  }
  return next;
};

const mergeSnapshots = (
  previous: UpstreamQuotaSnapshot[],
  next: UpstreamQuotaSnapshot[],
): UpstreamQuotaSnapshot[] => {
  const previousById = new Map(previous.map((item) => [item.account.account_id, item]));
  return next.map((item) => mergeSnapshot(previousById.get(item.account.account_id), item));
};

const resourceByKey = (snapshot: UpstreamQuotaSnapshot, key: string): QuotaResource | undefined => (
  snapshot.resources.find((resource) => resource.key === key)
);

const quotaTone = (remaining?: number | null): 'normal' | 'warning' | 'danger' => {
  if (remaining === undefined || remaining === null) return 'normal';
  if (remaining < 10) return 'danger';
  if (remaining < 20) return 'warning';
  return 'normal';
};

const statusTone = (status: UpstreamQuotaStatus): 'success' | 'warning' | 'danger' | 'muted' | 'accent' => {
  switch (status) {
    case 'ok': return 'success';
    case 'low': return 'warning';
    case 'exhausted':
    case 'refresh_failed':
    case 'auth_error': return 'danger';
    case 'refreshing': return 'accent';
    case 'unsupported':
    case 'disabled': return 'muted';
  }
};

const displayError = (error: unknown): string => (
  error instanceof Error ? error.message : String(error)
);

const formatCompactDateTime = (value: string): string => formatDateTime(value, {
  month: 'short',
  day: 'numeric',
  hour: '2-digit',
  minute: '2-digit',
});

const formatBalance = (resource: QuotaResource): string => {
  const value = resource.remaining;
  if (value === undefined || value === null) return '—';
  if (/^[A-Z]{3}$/.test(resource.unit)) {
    try {
      return new Intl.NumberFormat(undefined, {
        style: 'currency',
        currency: resource.unit,
        maximumFractionDigits: 2,
      }).format(value);
    } catch {
      // Fall back to a stable provider-unit rendering for unknown currencies.
    }
  }
  return `${resource.unit} ${value.toLocaleString()}`;
};

function WindowCell({ resource, unsupported, t }: {
  resource?: QuotaResource;
  unsupported: boolean;
  t: QuotaTranslation;
}) {
  if (!resource || resource.remaining === undefined || resource.remaining === null) {
    return <span className={styles.secondary}>{unsupported ? t('quota.not_applicable') : '—'}</span>;
  }
  const remaining = Math.max(0, Math.min(100, resource.remaining));
  const tone = quotaTone(remaining);
  return (
    <div className={styles.quotaCell}>
      <div className={styles.quotaValue}>
        <span>{t('quota.remaining')}</span>
        <strong>{Math.round(remaining)}%</strong>
      </div>
      <div className={styles.quotaTrack} data-tone={tone} aria-hidden="true">
        <span data-tone={tone} style={{ width: `${remaining}%` }} />
      </div>
      <span className={styles.quotaReset}>
        {resource.reset_at
          ? t('quota.reset_at', { time: formatCompactDateTime(resource.reset_at) })
          : t('quota.reset_unknown')}
      </span>
    </div>
  );
}

function SnapshotStatus({ snapshot, refreshing, t }: {
  snapshot: UpstreamQuotaSnapshot;
  refreshing: boolean;
  t: QuotaTranslation;
}) {
  if (refreshing) {
    return <StatusPill tone="accent">{t('quota.status.refreshing')}</StatusPill>;
  }
  const detail = snapshot.stale && snapshot.fetched_at
    ? t('quota.status_context.stale_snapshot', { time: formatCompactDateTime(snapshot.fetched_at) })
    : snapshot.status === 'exhausted'
      ? t('quota.status_context.exhausted_source')
      : undefined;
  return (
    <span className={styles.statusCell}>
      <StatusPill tone={statusTone(snapshot.status)}>
        {t(`quota.status.${snapshot.status}`)}
      </StatusPill>
      {detail && <small>{detail}</small>}
    </span>
  );
}

function SnapshotTime({ value }: { value?: string | null }) {
  if (!value) return <span className={styles.secondary}>—</span>;
  const exact = formatDateTime(value);
  return (
    <time className={styles.snapshotTime} dateTime={value} title={exact} aria-label={exact}>
      <span>{formatDateTime(value, { dateStyle: 'short' })}</span>
      <small>{formatDateTime(value, { timeStyle: 'short' })}</small>
    </time>
  );
}

function BalanceCell({ snapshot }: { snapshot: UpstreamQuotaSnapshot }) {
  const balances = snapshot.resources.filter((resource) => resource.type === 'balance');
  if (balances.length === 0) return <span className={styles.secondary}>—</span>;
  return (
    <div className={styles.account}>
      {balances.map((resource) => (
        <span key={resource.key} className={styles.balance}>{formatBalance(resource)}</span>
      ))}
    </div>
  );
}

function ListPage({ client, refreshRevision = 0, onBusyChange }: Omit<UpstreamQuotaPageProps, 'accountId'>) {
  const { t } = useTranslation('console');
  const [items, setItems] = useState<UpstreamQuotaSnapshot[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string>();
  const [refreshingAll, setRefreshingAll] = useState(false);
  const [refreshingIds, setRefreshingIds] = useState<ReadonlySet<string>>(() => new Set());
  const [refreshSummary, setRefreshSummary] = useState<string>();
  const [listRetry, setListRetry] = useState(0);
  const [search, setSearch] = useState('');
  const [provider, setProvider] = useState('all');
  const [status, setStatus] = useState('all');
  const [onlyProblems, setOnlyProblems] = useState(false);

  useEffect(() => {
    const controller = new AbortController();
    setLoading(true);
    setError(undefined);
    onBusyChange?.(true);
    void client.list(controller.signal)
      .then((next) => setItems((current) => mergeSnapshots(current, next)))
      .catch((cause) => {
        if (!controller.signal.aborted) setError(displayError(cause));
      })
      .finally(() => {
        if (!controller.signal.aborted) {
          setLoading(false);
          onBusyChange?.(false);
        }
      });
    return () => controller.abort();
  }, [client, listRetry, refreshRevision, onBusyChange]);

  const retryList = () => setListRetry((current) => current + 1);

  const refreshAll = async () => {
    if (refreshingAll) return;
    setRefreshingAll(true);
    setRefreshSummary(undefined);
    setError(undefined);
    onBusyChange?.(true);
    try {
      const next = await client.refreshAll();
      setItems((current) => mergeSnapshots(current, next));
      const failed = next.filter((item) => FAILED_STATUSES.has(item.status)).length;
      setRefreshSummary(t('quota.refresh_summary', {
        succeeded: next.length - failed,
        failed,
      }));
    } catch (cause) {
      setError(displayError(cause));
    } finally {
      setRefreshingAll(false);
      onBusyChange?.(false);
    }
  };

  const refreshOne = async (accountId: string) => {
    if (refreshingIds.has(accountId)) return;
    setRefreshingIds((current) => new Set(current).add(accountId));
    setError(undefined);
    try {
      const next = await client.refresh(accountId);
      setItems((current) => current.map((item) => (
        item.account.account_id === accountId ? mergeSnapshot(item, next) : item
      )));
    } catch (cause) {
      setError(displayError(cause));
    } finally {
      setRefreshingIds((current) => {
        const next = new Set(current);
        next.delete(accountId);
        return next;
      });
    }
  };

  const providers = useMemo(() => Array.from(new Set(items.map((item) => item.account.provider_id))).sort(), [items]);
  const filtered = useMemo(() => {
    const term = search.trim().toLowerCase();
    return items.filter((item) => {
      if (term && ![
        item.account.account_id,
        item.account.account_display_name,
        item.account.source_id,
        item.account.source_display_name,
      ].some((value) => value.toLowerCase().includes(term))) return false;
      if (provider !== 'all' && item.account.provider_id !== provider) return false;
      if (status !== 'all' && item.status !== status) return false;
      if (onlyProblems && !PROBLEM_STATUSES.has(item.status)) return false;
      return true;
    });
  }, [items, onlyProblems, provider, search, status]);

  const usable = items.filter((item) => item.status === 'ok' || item.status === 'low').length;
  const quotaAlerts = items.filter((item) => item.status === 'low' || item.status === 'exhausted').length;
  const failed = items.filter((item) => FAILED_STATUSES.has(item.status)).length;
  const enabled = items.filter((item) => item.account.enabled).length;

  if (loading && items.length === 0) return <LoadingState label={t('quota.loading')} />;

  if (error && items.length === 0) {
    return (
      <section className={styles.page} data-od-id="page-upstream-quotas">
        <Notice action={<Button size="sm" variant="secondary" onClick={retryList}><IconRefreshCw size={14} />{t('common.retry')}</Button>}>
          <strong>{t('quota.load_failed')}</strong>
          <span>{error}</span>
        </Notice>
      </section>
    );
  }

  if (items.length === 0) {
    return (
      <section className={styles.page} data-od-id="page-upstream-quotas">
        <EmptyState
          title={t('quota.empty_accounts_title')}
          description={t('quota.empty_accounts_description')}
          layout="centered"
          action={<Button variant="secondary" onClick={() => { window.location.hash = '#sources'; }}>{t('quota.configure_accounts')}</Button>}
        />
      </section>
    );
  }

  const resetFilters = () => {
    setSearch('');
    setProvider('all');
    setStatus('all');
    setOnlyProblems(false);
  };

  return (
    <section className={styles.page} data-od-id="page-upstream-quotas">
      {refreshSummary && <Notice tone={failed > 0 ? 'warning' : 'success'}>{refreshSummary}</Notice>}
      {error && (
        <Notice action={<Button size="sm" variant="secondary" onClick={retryList}><IconRefreshCw size={14} />{t('common.retry')}</Button>}>
          <strong>{t('quota.load_failed')}</strong>
          <span>{error}</span>
        </Notice>
      )}

      <div className={styles.summaryGrid}>
        <MetricCard compact label={t('quota.summary.accounts')} value={String(items.length)} hint={t('quota.summary.accounts_hint', { count: enabled })} />
        <MetricCard compact label={t('quota.summary.available')} value={String(usable)} hint={t('quota.summary.available_hint')} tone={usable === 0 && items.length > 0 ? 'warning' : undefined} />
        <MetricCard compact label={t('quota.summary.alerts')} value={String(quotaAlerts)} hint={t('quota.summary.alerts_hint')} tone={quotaAlerts > 0 ? 'warning' : undefined} />
        <MetricCard compact label={t('quota.summary.failed')} value={String(failed)} hint={t('quota.summary.failed_hint')} tone={failed > 0 ? 'warning' : undefined} />
      </div>

      <Card
        variant="flush"
        title={t('quota.list_title', { count: filtered.length })}
        extra={(
          <Button variant="secondary" loading={refreshingAll} onClick={() => void refreshAll()}>
            <IconRefreshCw size={14} />{t('quota.refresh_all')}
          </Button>
        )}
      >
        <div className={styles.toolbar}>
          <TextField
            className={styles.search}
            label={t('quota.search')}
            placeholder={t('quota.search_placeholder')}
            type="search"
            value={search}
            onChange={(event) => setSearch(event.target.value)}
            autoComplete="off"
          />
          <Select
            className={styles.select}
            label={t('quota.provider')}
            value={provider}
            onChange={(value) => setProvider(value ?? 'all')}
            data={[
              { value: 'all', label: t('quota.all') },
              ...providers.map((value) => ({ value, label: providerLabel(value) })),
            ]}
            allowDeselect={false}
          />
          <Select
            className={styles.select}
            label={t('quota.state')}
            value={status}
            onChange={(value) => setStatus(value ?? 'all')}
            data={[
              { value: 'all', label: t('quota.all') },
              ...(['ok', 'low', 'exhausted', 'refresh_failed', 'auth_error', 'unsupported', 'disabled'] as UpstreamQuotaStatus[])
                .map((value) => ({ value, label: t(`quota.status.${value}`) })),
            ]}
            allowDeselect={false}
          />
          <div className={styles.check}>
            <CheckboxField
              label={t('quota.only_problems')}
              checked={onlyProblems}
              onChange={setOnlyProblems}
            />
          </div>
        </div>

        {filtered.length === 0 ? (
          <EmptyState
            title={t('quota.empty_filtered_title')}
            description={t('quota.empty_filtered_description')}
            layout="centered"
            action={<Button variant="secondary" onClick={resetFilters}>{t('quota.reset_filters')}</Button>}
          />
        ) : <TableScroll label={t('quota.table_region')}>
          <Table className={styles.table}>
            <Table.Thead><Table.Tr>
              <Table.Th scope="col">{t('quota.column.account')}</Table.Th>
              <Table.Th scope="col">{t('quota.column.provider')}</Table.Th>
              <Table.Th scope="col">{t('quota.column.window_5h')}</Table.Th>
              <Table.Th scope="col">{t('quota.column.window_7d')}</Table.Th>
              <Table.Th scope="col">{t('quota.column.balance')}</Table.Th>
              <Table.Th scope="col">{t('quota.column.status')}</Table.Th>
              <Table.Th scope="col">{t('quota.column.updated')}</Table.Th>
              <Table.Th scope="col">{t('quota.column.actions')}</Table.Th>
            </Table.Tr></Table.Thead>
            <Table.Tbody>
              {filtered.map((snapshot) => {
                const accountId = snapshot.account.account_id;
                const refreshing = refreshingIds.has(accountId);
                const unsupportedWindows = snapshot.account.provider_id === 'deepseek' || snapshot.status === 'unsupported';
                return (
                  <Table.Tr key={accountId}>
                    <Table.Td>
                      <span className={styles.account}>
                        <strong>{snapshot.account.account_display_name}</strong>
                        <small>{accountId}</small>
                        <small>{snapshot.account.source_display_name}</small>
                      </span>
                    </Table.Td>
                    <Table.Td>{providerLabel(snapshot.account.provider_id)}</Table.Td>
                    <Table.Td><WindowCell resource={resourceByKey(snapshot, '5h')} unsupported={unsupportedWindows} t={t} /></Table.Td>
                    <Table.Td><WindowCell resource={resourceByKey(snapshot, '7d')} unsupported={unsupportedWindows} t={t} /></Table.Td>
                    <Table.Td><BalanceCell snapshot={snapshot} /></Table.Td>
                    <Table.Td><SnapshotStatus snapshot={snapshot} refreshing={refreshing} t={t} /></Table.Td>
                    <Table.Td><SnapshotTime value={snapshot.fetched_at} /></Table.Td>
                    <Table.Td>
                      <div className={styles.actions}>
                        <Button variant="ghost" size="sm" loading={refreshing} disabled={snapshot.status === 'unsupported' || snapshot.status === 'disabled'} onClick={() => void refreshOne(accountId)}>
                          <IconRefreshCw size={13} />{t('quota.refresh')}
                        </Button>
                        <Button variant="ghost" size="sm" onClick={() => { window.location.hash = `#upstream-quotas/${encodeURIComponent(accountId)}`; }}>
                          {t('quota.details')}
                        </Button>
                      </div>
                    </Table.Td>
                  </Table.Tr>
                );
              })}
            </Table.Tbody>
          </Table>
        </TableScroll>}
        <p className={styles.listNote}>{t('quota.snapshot_note')}</p>
      </Card>
    </section>
  );
}

function DetailPage({ client, accountId, refreshRevision = 0, onBusyChange }: UpstreamQuotaPageProps & { accountId: string }) {
  const { t } = useTranslation('console');
  const [snapshot, setSnapshot] = useState<UpstreamQuotaSnapshot>();
  const [loading, setLoading] = useState(true);
  const [refreshing, setRefreshing] = useState(false);
  const [error, setError] = useState<string>();
  const [attempts, setAttempts] = useState<UpstreamQuotaSnapshot[]>([]);
  const [detailRetry, setDetailRetry] = useState(0);

  const applySnapshot = useCallback((next: UpstreamQuotaSnapshot) => {
    setSnapshot((current) => mergeSnapshot(current, next));
    setAttempts((current) => [next, ...current].slice(0, 5));
  }, []);

  useEffect(() => {
    const controller = new AbortController();
    setLoading(true);
    setError(undefined);
    onBusyChange?.(true);
    void client.get(accountId, controller.signal)
      .then(applySnapshot)
      .catch((cause) => {
        if (!controller.signal.aborted) setError(displayError(cause));
      })
      .finally(() => {
        if (!controller.signal.aborted) {
          setLoading(false);
          onBusyChange?.(false);
        }
      });
    return () => controller.abort();
  }, [accountId, applySnapshot, client, detailRetry, onBusyChange, refreshRevision]);

  const refresh = async () => {
    if (refreshing) return;
    setRefreshing(true);
    setError(undefined);
    onBusyChange?.(true);
    try {
      applySnapshot(await client.refresh(accountId));
    } catch (cause) {
      setError(displayError(cause));
    } finally {
      setRefreshing(false);
      onBusyChange?.(false);
    }
  };

  if (loading && !snapshot) return <LoadingState label={t('quota.loading')} />;
  if (!snapshot) {
    return (
      <section className={styles.page} data-od-id="page-upstream-quota-detail">
        <div>
          <Button variant="ghost" size="sm" onClick={() => { window.location.hash = '#upstream-quotas'; }}>
            ← {t('quota.back')}
          </Button>
        </div>
        <Notice action={<Button size="sm" variant="secondary" onClick={() => setDetailRetry((current) => current + 1)}><IconRefreshCw size={14} />{t('common.retry')}</Button>}>
          <strong>{error ? t('quota.load_failed') : t('quota.missing')}</strong>
          {error && <span>{error}</span>}
        </Notice>
      </section>
    );
  }

  const windows = snapshot.resources.filter((resource) => resource.type === 'window');
  const balances = snapshot.resources.filter((resource) => resource.type === 'balance');

  return (
    <section className={styles.page} data-od-id="page-upstream-quota-detail">
      <div>
        <Button variant="ghost" size="sm" onClick={() => { window.location.hash = '#upstream-quotas'; }}>
          ← {t('quota.back')}
        </Button>
      </div>
      {error && (
        <Notice action={<Button size="sm" variant="secondary" onClick={() => setDetailRetry((current) => current + 1)}><IconRefreshCw size={14} />{t('common.retry')}</Button>}>
          <strong>{t('quota.load_failed')}</strong>
          <span>{error}</span>
        </Notice>
      )}
      {snapshot.stale && snapshot.refresh_error && (
        <Notice tone="warning">
          {t('quota.stale_warning', { message: snapshot.refresh_error.message })}
        </Notice>
      )}

      <Card>
        <div className={styles.detailHeader}>
          <div className={styles.detailIdentity}>
            <h2>{snapshot.account.account_display_name}</h2>
            <span className={styles.secondary}>{snapshot.account.account_id} · {providerLabel(snapshot.account.provider_id)}</span>
            <div className={styles.detailMeta}>
              <SnapshotStatus snapshot={snapshot} refreshing={refreshing} t={t} />
              <span className={styles.secondary}>{snapshot.account.source_display_name}</span>
              <span className={styles.secondary}>{snapshot.fetched_at ? formatDateTime(snapshot.fetched_at) : '—'}</span>
            </div>
          </div>
          <Button variant="secondary" loading={refreshing} disabled={snapshot.status === 'unsupported' || snapshot.status === 'disabled'} onClick={() => void refresh()}>
            <IconRefreshCw size={14} />{t('quota.refresh')}
          </Button>
        </div>
      </Card>

      <div className={styles.detailGrid}>
        <div className={styles.stack}>
          <Card title={balances.length > 0 && windows.length === 0 ? t('quota.current_balance') : t('quota.current_quota')}>
            {windows.length > 0 && (
              <div className={styles.resourceList}>
                {windows.map((resource) => {
                  const remaining = resource.remaining ?? 0;
                  return (
                    <div className={styles.resourceBlock} key={resource.key}>
                      <div className={styles.resourceHeading}>
                        <strong>{resource.label}</strong>
                        <span>{t('quota.remaining')} {Math.round(remaining)}%</span>
                      </div>
                      <div className={styles.quotaTrack} data-tone={quotaTone(remaining)}>
                        <span data-tone={quotaTone(remaining)} style={{ width: `${Math.max(0, Math.min(100, remaining))}%` }} />
                      </div>
                      <div className={styles.resourceMeta}>
                        <span>{t('quota.used')} {Math.round(resource.used ?? (100 - remaining))}%</span>
                        <span>{resource.reset_at ? t('quota.reset_at', { time: formatDateTime(resource.reset_at) }) : t('quota.reset_unknown')}</span>
                      </div>
                    </div>
                  );
                })}
              </div>
            )}
            {balances.length > 0 && (
              <div className={styles.resourceList}>
                {balances.map((resource) => (
                  <div className={styles.balanceHero} key={resource.key}>
                    <span className={styles.secondary}>{resource.label}</span>
                    <strong>{formatBalance(resource)}</strong>
                  </div>
                ))}
              </div>
            )}
            {windows.length === 0 && balances.length === 0 && <p className={styles.note}>{t('quota.no_resources')}</p>}
          </Card>

          <Card title={t('quota.session_attempts')}>
            <TableScroll label={t('quota.session_attempts')}>
              <Table className={styles.attemptTable}>
                <Table.Thead><Table.Tr>
                  <Table.Th scope="col">{t('quota.column.updated')}</Table.Th>
                  <Table.Th scope="col">{t('quota.column.status')}</Table.Th>
                  <Table.Th scope="col">{t('quota.latency')}</Table.Th>
                </Table.Tr></Table.Thead>
                <Table.Tbody>
                  {attempts.map((attempt, index) => (
                    <Table.Tr key={`${attempt.attempted_at}-${index}`}>
                      <Table.Td className={styles.attemptTime}>{formatDateTime(attempt.attempted_at)}</Table.Td>
                      <Table.Td><StatusPill className={styles.attemptStatus} tone={statusTone(attempt.status)}>{t(`quota.status.${attempt.status}`)}</StatusPill></Table.Td>
                      <Table.Td className={styles.attemptLatency}>{attempt.latency_ms} ms</Table.Td>
                    </Table.Tr>
                  ))}
                </Table.Tbody>
              </Table>
            </TableScroll>
            <p className={styles.note}>{t('quota.session_attempts_note')}</p>
          </Card>
        </div>

        <div className={styles.stack}>
          <Card title={t('quota.account_info')}>
            <DetailList>
              <DetailItem label={t('quota.provider')}>{providerLabel(snapshot.account.provider_id)}</DetailItem>
              <DetailItem label={t('quota.source')}>{snapshot.account.source_display_name} · {snapshot.account.source_id}</DetailItem>
              <DetailItem label={t('quota.column.account')}>{snapshot.account.account_id}</DetailItem>
              <DetailItem label={t('quota.enabled')}>{snapshot.account.enabled ? t('common.yes') : t('common.no')}</DetailItem>
            </DetailList>
          </Card>

          <Card title={t('quota.data_state')}>
            <DetailList>
              <DetailItem label={t('quota.column.status')}>{t(`quota.status.${snapshot.status}`)}</DetailItem>
              <DetailItem label={t('quota.fetched_at')}>{snapshot.fetched_at ? formatDateTime(snapshot.fetched_at) : '—'}</DetailItem>
              <DetailItem label={t('quota.attempted_at')}>{formatDateTime(snapshot.attempted_at)}</DetailItem>
              <DetailItem label={t('quota.latency')}>{snapshot.latency_ms} ms</DetailItem>
              <DetailItem label={t('quota.data_source')}>{t('quota.data_source_upstream_api')}</DetailItem>
            </DetailList>
          </Card>
        </div>
      </div>

      <Card title={t('quota.raw_data')}>
        <pre className={styles.raw}>{snapshot.raw ? JSON.stringify(snapshot.raw, null, 2) : t('quota.raw_unavailable')}</pre>
      </Card>
    </section>
  );
}

export function UpstreamQuotaPage(props: UpstreamQuotaPageProps) {
  return props.accountId
    ? <DetailPage {...props} accountId={props.accountId} />
    : <ListPage {...props} />;
}
