import {
  RUNTIME_EVENT_CATEGORIES,
  RUNTIME_EVENT_LEVELS,
  RUNTIME_EVENT_SOURCES,
  isAbortError,
  normalizeAdminError,
  type AdminErrorShape,
  type GatewayAdminResources,
  type RuntimeEventCategory,
  type RuntimeEventFilters,
  type RuntimeEventLevel,
  type RuntimeEventRecord,
  type RuntimeEventResponse,
  type RuntimeEventSource,
} from '@/admin-api';
import { Button } from '@/components/ui/Button';
import { Card } from '@/components/ui/Card';
import { DetailItem, DetailList } from '@/components/ui/DetailList';
import { EmptyState } from '@/components/ui/EmptyState';
import { FilterPanel } from '@/components/ui/FilterPanel';
import { SelectField, TextField } from '@/components/ui/FormField';
import { RemoteFilterField } from '@/components/ui/RemoteFilterField';
import { IconButton } from '@/components/ui/IconButton';
import { IconEye, IconRefreshCw } from '@/components/ui/icons';
import { LoadingState } from '@/components/ui/LoadingState';
import { Modal } from '@/components/ui/Modal';
import { Notice } from '@/components/ui/Notice';
import { StatusPill, type StatusTone } from '@/components/ui/StatusPill';
import { TableScroll } from '@/components/ui/TableScroll';
import { useAdminQuery } from '@/hooks/useAdminQuery';
import type { QuerySession } from '@/hooks/useQuerySession';
import { formatDateTime } from '@/utils/format';
import { sourceRouteHash, upstreamQuotaRouteHash, usageEventRouteHash } from '@/lib/consoleNavigation';
import { Table } from '@mantine/core';
import { useCallback, useMemo, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import styles from './RuntimeEventsPage.module.scss';

interface RuntimeEventsPageProps {
  api: GatewayAdminResources;
  refreshRevision?: number;
  onBusyChange?: (busy: boolean) => void;
  initialCorrelationId?: string;
}

interface DraftFilters {
  category: '' | RuntimeEventCategory;
  level: '' | RuntimeEventLevel;
  source: '' | RuntimeEventSource;
  eventType: string;
  subjectId: string;
  correlationId: string;
  from: string;
  to: string;
}

interface AppendedPageState {
  base?: RuntimeEventResponse;
  data: RuntimeEventRecord[];
  hasMore: boolean;
  nextCursor?: string | null;
}

const EMPTY_FILTERS: DraftFilters = {
  category: '',
  level: '',
  source: '',
  eventType: '',
  subjectId: '',
  correlationId: '',
  from: '',
  to: '',
};

const isoTimestamp = (value: string): string | undefined => {
  if (!value) return undefined;
  const parsed = new Date(value);
  return Number.isNaN(parsed.getTime()) ? undefined : parsed.toISOString();
};

const initialDraftFilters = (correlationId?: string): DraftFilters => ({
  ...EMPTY_FILTERS,
  correlationId: correlationId?.trim() ?? '',
});

const appliedFilters = (draft: DraftFilters): RuntimeEventFilters => ({
  category: draft.category || undefined,
  level: draft.level || undefined,
  source: draft.source || undefined,
  event_type: draft.eventType.trim() || undefined,
  subject_id: draft.subjectId.trim() || undefined,
  correlation_id: draft.correlationId.trim() || undefined,
  from: isoTimestamp(draft.from),
  to: isoTimestamp(draft.to),
  limit: 100,
});

const levelTone = (level: RuntimeEventLevel): StatusTone => {
  if (level === 'error') return 'danger';
  if (level === 'warning') return 'warning';
  return 'accent';
};

const detailText = (event: RuntimeEventRecord, key: string): string | undefined => {
  const value = event.details[key];
  return typeof value === 'string' && value.trim() ? value : typeof value === 'number' ? String(value) : undefined;
};

function CopyableId({ label, value }: { label: string; value?: string | null }) {
  const { t } = useTranslation('console');
  const [result, setResult] = useState<'copied' | 'failed'>();
  if (!value) return <DetailItem label={label}>—</DetailItem>;
  return <DetailItem label={label}>
    <span className={styles.copyId}><code>{value}</code><Button size="sm" variant="ghost" onClick={async () => {
      try { await navigator.clipboard.writeText(value); setResult('copied'); }
      catch { setResult('failed'); }
    }}>{t('runtimeEvents.copy_id')}</Button></span>
    {result && <span role="status" className={styles.copyFeedback}>{t(`runtimeEvents.copy_${result}`)}</span>}
  </DetailItem>;
}

function QueryError({ error, retry }: { error: AdminErrorShape; retry: () => void }) {
  const { t } = useTranslation('console');
  return (
    <Notice action={<Button size="sm" variant="secondary" onClick={retry}>{t('common.retry')}</Button>}>
      <strong>{t('runtimeEvents.query_failed')}</strong>
      {error.code && <code>{error.code}</code>}
    </Notice>
  );
}

function EventDetails({ event, onClose, onRelated }: { event: RuntimeEventRecord; onClose: () => void; onRelated: (id: string) => void }) {
  const { t } = useTranslation('console');
  const [open, setOpen] = useState(true);
  const [related, setRelated] = useState<string>();
  const sourceId = detailText(event, 'source_id') ?? (event.subject_type === 'source' ? event.subject_id : undefined);
  const accountId = detailText(event, 'account_id') ?? (event.subject_type === 'account' ? event.subject_id : undefined);
  const requestId = event.subject_type === 'request' ? event.subject_id : undefined;
  return (
    <Modal
      open={open}
      variant="drawer"
      width={620}
      title={t('runtimeEvents.detail_title')}
      onClose={() => setOpen(false)}
      onExitTransitionEnd={() => { onClose(); if (related) onRelated(related); }}
      footer={<Button variant="secondary" onClick={() => setOpen(false)}>{t('common.close')}</Button>}
    >
      <DetailList>
        <CopyableId label={t('runtimeEvents.event_id')} value={event.event_id} />
        <DetailItem label={t('runtimeEvents.occurred_at')}><time dateTime={event.occurred_at}>{formatDateTime(event.occurred_at)}</time></DetailItem>
        <DetailItem label={t('runtimeEvents.level')}><StatusPill tone={levelTone(event.level)}>{t(`runtimeEvents.levels.${event.level}`)}</StatusPill></DetailItem>
        <DetailItem label={t('runtimeEvents.category')}><StatusPill>{t(`runtimeEvents.categories.${event.category}`)}</StatusPill></DetailItem>
        <DetailItem label={t('runtimeEvents.event_type')}>{t(`runtimeEvents.eventTypes.${event.event_type}`, { defaultValue: event.event_type })}</DetailItem>
        <DetailItem label={t('runtimeEvents.message')}>{event.message}</DetailItem>
        <CopyableId label={t('runtimeEvents.subject_id')} value={event.subject_id} />
        <CopyableId label={t('runtimeEvents.correlation_id')} value={event.correlation_id} />
        <DetailItem label={t('runtimeEvents.fact_source')}>{t(`runtimeEvents.sources.${event.source}`, { defaultValue: event.source })}</DetailItem>
      </DetailList>
      <div className={styles.detailActions}>
        {event.correlation_id && event.correlation_id !== '[REDACTED]' && <Button size="sm" variant="secondary" onClick={() => { setRelated(event.correlation_id!); setOpen(false); }}>{t('runtimeEvents.related_events')}</Button>}
        {sourceId && <Button size="sm" variant="secondary" onClick={() => { window.location.hash = sourceRouteHash(sourceId); }}>{t('runtimeEvents.open_source')}</Button>}
        {accountId && <Button size="sm" variant="secondary" onClick={() => { window.location.hash = upstreamQuotaRouteHash(accountId); }}>{t('runtimeEvents.open_account_quota')}</Button>}
        {requestId && <Button size="sm" variant="secondary" onClick={() => { window.location.hash = usageEventRouteHash(requestId); }}>{t('runtimeEvents.open_request')}</Button>}
      </div>
      <details className={styles.detailsSection}>
        <summary>{t('runtimeEvents.technical_details')}</summary>
        <DetailList>
          <DetailItem label={t('runtimeEvents.event_type')}><code>{event.event_type}</code></DetailItem>
          <DetailItem label={t('runtimeEvents.subject_type')}><code>{event.subject_type}</code></DetailItem>
        </DetailList>
        <pre>{JSON.stringify(event.details, null, 2)}</pre>
      </details>
    </Modal>
  );
}

export function RuntimeEventsPage({ api, refreshRevision = 0, onBusyChange, initialCorrelationId }: RuntimeEventsPageProps) {
  const { t } = useTranslation('console');
  const initialDraft = useMemo(() => initialDraftFilters(initialCorrelationId), [initialCorrelationId]);
  const [draft, setDraft] = useState<DraftFilters>(initialDraft);
  const optionFrom = isoTimestamp(draft.from);
  const optionTo = isoTimestamp(draft.to);
  const loadEventTypes = useCallback((search: string, signal: AbortSignal) =>
    api.eventTypeOptions({ from: optionFrom, to: optionTo }, search, signal), [api, optionFrom, optionTo]);
  const [filters, setFilters] = useState<RuntimeEventFilters>(() => appliedFilters(initialDraft));
  const [timeRangeError, setTimeRangeError] = useState('');
  const [selected, setSelected] = useState<RuntimeEventRecord>();
  const timezone = Intl.DateTimeFormat().resolvedOptions().timeZone || 'UTC';
  const [appended, setAppended] = useState<AppendedPageState>({ data: [], hasMore: false });
  const [loadingMoreSession, setLoadingMoreSession] = useState<QuerySession<RuntimeEventResponse>>();
  const [loadMoreError, setLoadMoreError] = useState<{ session: QuerySession<RuntimeEventResponse>; error: AdminErrorShape }>();
  const pendingPage = useRef<QuerySession<RuntimeEventResponse> | undefined>(undefined);

  const clearPagination = useCallback(() => {
    pendingPage.current = undefined;
    setLoadingMoreSession(undefined);
    setLoadMoreError(undefined);
    setAppended({ data: [], hasMore: false });
  }, []);

  const queryKey = useMemo(() => JSON.stringify(filters), [filters]);
  const load = useCallback(
    (signal: AbortSignal) => api.runtimeEvents(filters, signal),
    [api, filters],
  );
  const query = useAdminQuery({ load, queryKey, refreshRevision, onBusyChange });
  const loadingMore = loadingMoreSession !== undefined && loadingMoreSession === query.session;
  const response = query.data;
  const appendedForResponse = response && appended.base === response ? appended : undefined;
  const rows = useMemo(() => {
    if (!response) return [];
    const combined = [...response.data, ...(appendedForResponse?.data ?? [])];
    return [...new Map(combined.map((event) => [event.event_id, event])).values()];
  }, [appendedForResponse?.data, response]);
  const hasMore = appendedForResponse?.hasMore ?? response?.page.has_more ?? false;
  const nextCursor = appendedForResponse?.nextCursor ?? response?.page.next_cursor;
  const visibleLoadMoreError = loadMoreError?.session === query.session ? loadMoreError?.error : undefined;

  const apply = () => {
    const from = draft.from ? new Date(draft.from) : undefined;
    const to = draft.to ? new Date(draft.to) : undefined;
    if ((from && Number.isNaN(from.getTime())) || (to && Number.isNaN(to.getTime()))) {
      setTimeRangeError(t('runtimeEvents.invalid_time'));
      return;
    }
    if (from && to && from >= to) {
      setTimeRangeError(t('runtimeEvents.invalid_range'));
      return;
    }
    setTimeRangeError('');
    clearPagination();
    setSelected(undefined);
    setFilters(appliedFilters(draft));
  };
  const reset = () => {
    clearPagination();
    const empty = { ...EMPTY_FILTERS };
    setTimeRangeError('');
    setDraft(empty);
    setSelected(undefined);
    setFilters(appliedFilters(empty));
  };
  const reload = query.reload;
  const loadMore = async () => {
    const session = query.getSession();
    if (!session || !hasMore || !nextCursor || query.loading || query.refreshing || pendingPage.current === session) return;
    pendingPage.current = session;
    setLoadingMoreSession(session);
    setLoadMoreError(undefined);
    try {
      const next = await api.runtimeEvents({ ...filters, cursor: nextCursor }, session.signal);
      if (!session.isCurrent()) return;
      setAppended((current) => session.isCurrent() ? ({
        base: session.data,
        data: [...(current.base === session.data ? current.data : []), ...next.data],
        hasMore: next.page.has_more,
        nextCursor: next.page.next_cursor,
      }) : current);
    } catch (error) {
      if (session.isCurrent() && !isAbortError(error)) {
        setLoadMoreError({ session, error: normalizeAdminError(error) });
      }
    } finally {
      if (pendingPage.current === session) {
        pendingPage.current = undefined;
        setLoadingMoreSession((current) => current === session ? undefined : current);
      }
    }
  };

  return (
    <section className={styles.page} data-od-id="page-runtime-events">
      <div className={styles.pageActions}>
        <div className={styles.feedMeta}>
          <span>{t('runtimeEvents.local_time', { timezone })}</span>
        </div>
        <Button variant="secondary" onClick={reload} loading={query.refreshing}>
          <IconRefreshCw size={14} />{t('runtimeEvents.refresh')}
        </Button>
      </div>

      <FilterPanel label={t('runtimeEvents.filters_aria')} className={styles.filters}>
        <SelectField
          label={t('runtimeEvents.category')}
          value={draft.category}
          data={[
            { value: '', label: t('runtimeEvents.categories.all') },
            ...RUNTIME_EVENT_CATEGORIES.map((category) => ({ value: category, label: t(`runtimeEvents.categories.${category}`) })),
          ]}
          onChange={(category) => setDraft((current) => ({ ...current, category: category as DraftFilters['category'] }))}
        />
        <SelectField
          label={t('runtimeEvents.level')}
          value={draft.level}
          data={[
            { value: '', label: t('runtimeEvents.levels.all') },
            ...RUNTIME_EVENT_LEVELS.map((level) => ({ value: level, label: t(`runtimeEvents.levels.${level}`) })),
          ]}
          onChange={(level) => setDraft((current) => ({ ...current, level: level as DraftFilters['level'] }))}
        />
        <SelectField
          label={t('runtimeEvents.source')}
          value={draft.source}
          data={[
            { value: '', label: t('runtimeEvents.sources.all') },
            ...RUNTIME_EVENT_SOURCES.map((source) => ({ value: source, label: t(`runtimeEvents.sources.${source}`) })),
          ]}
          onChange={(source) => setDraft((current) => ({ ...current, source: source as DraftFilters['source'] }))}
        />
        <RemoteFilterField label={t('runtimeEvents.event_type')} value={draft.eventType} loadOptions={loadEventTypes} contextKey={String(refreshRevision)} onChange={(eventType) => setDraft((current) => ({ ...current, eventType }))} />
        <TextField label={t('runtimeEvents.subject')} value={draft.subjectId} placeholder={t('runtimeEvents.subject_placeholder')} onChange={(event) => setDraft((current) => ({ ...current, subjectId: event.currentTarget.value }))} />
        <TextField label={t('runtimeEvents.correlation')} value={draft.correlationId} placeholder={t('runtimeEvents.correlation_placeholder')} onChange={(event) => setDraft((current) => ({ ...current, correlationId: event.currentTarget.value }))} />
        <TextField type="datetime-local" label={t('runtimeEvents.from')} value={draft.from} error={timeRangeError || undefined} onChange={(event) => { const value = event.currentTarget.value; setTimeRangeError(''); setDraft((current) => ({ ...current, from: value })); }} />
        <TextField type="datetime-local" label={t('runtimeEvents.to')} value={draft.to} error={timeRangeError || undefined} onChange={(event) => { const value = event.currentTarget.value; setTimeRangeError(''); setDraft((current) => ({ ...current, to: value })); }} />
        <div className={styles.filterActions}>
          <Button variant="secondary" onClick={reset}>{t('runtimeEvents.reset')}</Button>
          <Button onClick={apply}>{t('common.apply')}</Button>
        </div>
      </FilterPanel>

      {query.error && <QueryError error={query.error} retry={reload} />}

      <Card
        variant="flush"
        title={t('runtimeEvents.title')}
        subtitle={t('runtimeEvents.subtitle')}
        titleMeta={<StatusPill>{t('runtimeEvents.count', { count: rows.length })}</StatusPill>}
      >
        {query.loading && !response ? <LoadingState label={t('runtimeEvents.loading')} /> : !response ? null : rows.length === 0 ? (
          <EmptyState title={t('runtimeEvents.empty_title')} description={t('runtimeEvents.empty_description')} layout="centered" />
        ) : (
          <TableScroll label={t('runtimeEvents.title')}>
            <Table className={styles.table}>
              <Table.Thead>
                <Table.Tr>
                  <Table.Th scope="col">{t('common.actions')}</Table.Th>
                  <Table.Th scope="col">{t('runtimeEvents.column_time')}</Table.Th>
                  <Table.Th scope="col">{t('runtimeEvents.column_level')}</Table.Th>
                  <Table.Th scope="col">{t('runtimeEvents.column_event')}</Table.Th>
                  <Table.Th scope="col">{t('runtimeEvents.column_context')}</Table.Th>
                  <Table.Th scope="col">{t('runtimeEvents.column_source')}</Table.Th>
                </Table.Tr>
              </Table.Thead>
              <Table.Tbody>
                {rows.map((event) => (
                  <Table.Tr key={event.event_id}>
                    <Table.Td><IconButton label={t('runtimeEvents.view_aria', { id: event.event_id })} onClick={() => setSelected(event)}><IconEye size={16} /></IconButton></Table.Td>
                    <Table.Td><time dateTime={event.occurred_at}>{formatDateTime(event.occurred_at)}</time></Table.Td>
                    <Table.Td><StatusPill tone={levelTone(event.level)}>{t(`runtimeEvents.levels.${event.level}`)}</StatusPill></Table.Td>
                    <Table.Td><span className={styles.eventCell}><strong>{t(`runtimeEvents.eventTypes.${event.event_type}`, { defaultValue: event.event_type })}</strong><small>{detailText(event, 'error_summary') ?? event.message}</small><StatusPill>{t(`runtimeEvents.categories.${event.category}`, { defaultValue: event.category })}</StatusPill></span></Table.Td>
                    <Table.Td><span className={styles.stack}>{detailText(event, 'status_code') && <strong>HTTP {detailText(event, 'status_code')}</strong>}{detailText(event, 'logical_model') && <span>{t('runtimeEvents.model_context', { model: detailText(event, 'logical_model') })}</span>}{detailText(event, 'source_id') && <small>{t('runtimeEvents.source_context', { source: detailText(event, 'source_id') })}</small>}{detailText(event, 'account_id') && <small>{t('runtimeEvents.account_context', { account: detailText(event, 'account_id') })}</small>}{!detailText(event, 'status_code') && !detailText(event, 'logical_model') && !detailText(event, 'source_id') && !detailText(event, 'account_id') && <span>{event.subject_id && event.subject_id !== event.correlation_id ? event.subject_id : '—'}</span>}</span></Table.Td>
                    <Table.Td>{t(`runtimeEvents.sources.${event.source}`, { defaultValue: event.source })}</Table.Td>
                  </Table.Tr>
                ))}
              </Table.Tbody>
            </Table>
          </TableScroll>
        )}
        {(hasMore || visibleLoadMoreError) && (
          <div className={styles.loadMore}>
            {visibleLoadMoreError && <Notice><strong>{t('runtimeEvents.load_more_failed')}</strong>{visibleLoadMoreError.code && <code>{visibleLoadMoreError.code}</code>}</Notice>}
            <Button variant="secondary" loading={loadingMore} disabled={!hasMore} onClick={() => void loadMore()}>{t('runtimeEvents.load_more')}</Button>
          </div>
        )}
      </Card>

      {selected && <EventDetails key={selected.event_id} event={selected} onClose={() => setSelected(undefined)} onRelated={(id) => { const next = { ...EMPTY_FILTERS, correlationId: id }; clearPagination(); setDraft(next); setFilters(appliedFilters(next)); }} />}
    </section>
  );
}
