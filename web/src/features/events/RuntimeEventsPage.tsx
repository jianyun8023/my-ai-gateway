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
import { IconButton } from '@/components/ui/IconButton';
import { IconEye, IconRefreshCw } from '@/components/ui/icons';
import { LoadingState } from '@/components/ui/LoadingState';
import { Modal } from '@/components/ui/Modal';
import { Notice } from '@/components/ui/Notice';
import { StatusPill, type StatusTone } from '@/components/ui/StatusPill';
import { TableScroll } from '@/components/ui/TableScroll';
import { useAdminQuery } from '@/hooks/useAdminQuery';
import { formatDateTime } from '@/utils/format';
import { Table } from '@mantine/core';
import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import styles from './RuntimeEventsPage.module.scss';

interface RuntimeEventsPageProps {
  api: GatewayAdminResources;
  refreshRevision?: number;
  onBusyChange?: (busy: boolean) => void;
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

function QueryError({ error, retry }: { error: AdminErrorShape; retry: () => void }) {
  const { t } = useTranslation('console');
  return (
    <Notice action={<Button size="sm" variant="secondary" onClick={retry}>{t('common.retry')}</Button>}>
      <strong>{t('runtimeEvents.query_failed')}</strong>
      {error.code && <code>{error.code}</code>}
    </Notice>
  );
}

function EventDetails({ event, onClose }: { event: RuntimeEventRecord; onClose: () => void }) {
  const { t } = useTranslation('console');
  const [open, setOpen] = useState(true);
  return (
    <Modal
      open={open}
      variant="drawer"
      width={620}
      title={t('runtimeEvents.detail_title')}
      onClose={() => setOpen(false)}
      onExitTransitionEnd={onClose}
      footer={<Button variant="secondary" onClick={() => setOpen(false)}>{t('common.close')}</Button>}
    >
      <DetailList>
        <DetailItem label={t('runtimeEvents.event_id')}><code>{event.event_id}</code></DetailItem>
        <DetailItem label={t('runtimeEvents.occurred_at')}><time dateTime={event.occurred_at}>{formatDateTime(event.occurred_at)}</time></DetailItem>
        <DetailItem label={t('runtimeEvents.level')}><StatusPill tone={levelTone(event.level)}>{t(`runtimeEvents.levels.${event.level}`)}</StatusPill></DetailItem>
        <DetailItem label={t('runtimeEvents.category')}><StatusPill>{t(`runtimeEvents.categories.${event.category}`)}</StatusPill></DetailItem>
        <DetailItem label={t('runtimeEvents.event_type')}><code>{event.event_type}</code></DetailItem>
        <DetailItem label={t('runtimeEvents.message')}>{event.message}</DetailItem>
        <DetailItem label={t('runtimeEvents.subject_type')}><code>{event.subject_type}</code></DetailItem>
        <DetailItem label={t('runtimeEvents.subject_id')}><code>{event.subject_id ?? '—'}</code></DetailItem>
        <DetailItem label={t('runtimeEvents.correlation_id')}><code>{event.correlation_id ?? '—'}</code></DetailItem>
        <DetailItem label={t('runtimeEvents.fact_source')}><code>{event.source}</code></DetailItem>
      </DetailList>
      <section className={styles.detailsSection}>
        <h3>{t('runtimeEvents.details')}</h3>
        <pre>{JSON.stringify(event.details, null, 2)}</pre>
      </section>
    </Modal>
  );
}

export function RuntimeEventsPage({ api, refreshRevision = 0, onBusyChange }: RuntimeEventsPageProps) {
  const { t } = useTranslation('console');
  const [draft, setDraft] = useState<DraftFilters>({ ...EMPTY_FILTERS });
  const [filters, setFilters] = useState<RuntimeEventFilters>(() => appliedFilters(EMPTY_FILTERS));
  const [selected, setSelected] = useState<RuntimeEventRecord>();
  const [appended, setAppended] = useState<AppendedPageState>({ data: [], hasMore: false });
  const [loadingMore, setLoadingMore] = useState(false);
  const [loadMoreError, setLoadMoreError] = useState<{ base: RuntimeEventResponse; error: AdminErrorShape }>();
  const loadMoreController = useRef<AbortController | undefined>(undefined);
  useEffect(() => () => loadMoreController.current?.abort(), []);

  const clearPagination = useCallback(() => {
    loadMoreController.current?.abort();
    loadMoreController.current = undefined;
    setLoadingMore(false);
    setLoadMoreError(undefined);
    setAppended({ data: [], hasMore: false });
  }, []);

  const queryKey = useMemo(() => JSON.stringify(filters), [filters]);
  const load = useCallback(
    (signal: AbortSignal) => api.runtimeEvents(filters, signal),
    [api, filters],
  );
  const query = useAdminQuery({ load, queryKey, refreshRevision, onBusyChange });
  const response = query.data;
  const appendedForResponse = response && appended.base === response ? appended : undefined;
  const rows = useMemo(() => {
    if (!response) return [];
    const combined = [...response.data, ...(appendedForResponse?.data ?? [])];
    return [...new Map(combined.map((event) => [event.event_id, event])).values()];
  }, [appendedForResponse?.data, response]);
  const hasMore = appendedForResponse?.hasMore ?? response?.page.has_more ?? false;
  const nextCursor = appendedForResponse?.nextCursor ?? response?.page.next_cursor;
  const visibleLoadMoreError = response && loadMoreError?.base === response ? loadMoreError.error : undefined;

  useEffect(() => {
    clearPagination();
  }, [clearPagination, response]);

  const apply = () => {
    clearPagination();
    setSelected(undefined);
    setFilters(appliedFilters(draft));
  };
  const reset = () => {
    clearPagination();
    const empty = { ...EMPTY_FILTERS };
    setDraft(empty);
    setSelected(undefined);
    setFilters(appliedFilters(empty));
  };
  const reload = () => {
    clearPagination();
    query.reload();
  };
  const loadMore = async () => {
    if (!response || !hasMore || !nextCursor || loadingMore) return;
    loadMoreController.current?.abort();
    const controller = new AbortController();
    loadMoreController.current = controller;
    setLoadingMore(true);
    setLoadMoreError(undefined);
    try {
      const next = await api.runtimeEvents({ ...filters, cursor: nextCursor }, controller.signal);
      if (controller.signal.aborted) return;
      setAppended((current) => ({
        base: response,
        data: [...(current.base === response ? current.data : []), ...next.data],
        hasMore: next.page.has_more,
        nextCursor: next.page.next_cursor,
      }));
    } catch (error) {
      if (!controller.signal.aborted && !isAbortError(error)) {
        setLoadMoreError({ base: response, error: normalizeAdminError(error) });
      }
    } finally {
      if (loadMoreController.current === controller) {
        loadMoreController.current = undefined;
        setLoadingMore(false);
      }
    }
  };

  if (query.loading && !response) return <LoadingState label={t('runtimeEvents.loading')} />;
  if (query.error && !response) return <QueryError error={query.error} retry={query.reload} />;
  if (!response) return null;

  return (
    <section className={styles.page} data-od-id="page-runtime-events">
      <div className={styles.pageActions}>
        <div className={styles.feedMeta}>
          <StatusPill tone="accent">{response.version}</StatusPill>
          <StatusPill>{response.fact_source}</StatusPill>
          <span>{t('runtimeEvents.range', { boundary: response.range.boundary })}</span>
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
        <TextField label={t('runtimeEvents.event_type')} value={draft.eventType} placeholder={t('runtimeEvents.event_type_placeholder')} onChange={(event) => setDraft((current) => ({ ...current, eventType: event.currentTarget.value }))} />
        <TextField label={t('runtimeEvents.subject')} value={draft.subjectId} placeholder={t('runtimeEvents.subject_placeholder')} onChange={(event) => setDraft((current) => ({ ...current, subjectId: event.currentTarget.value }))} />
        <TextField label={t('runtimeEvents.correlation')} value={draft.correlationId} placeholder={t('runtimeEvents.correlation_placeholder')} onChange={(event) => setDraft((current) => ({ ...current, correlationId: event.currentTarget.value }))} />
        <TextField type="datetime-local" label={t('runtimeEvents.from')} value={draft.from} onChange={(event) => setDraft((current) => ({ ...current, from: event.currentTarget.value }))} />
        <TextField type="datetime-local" label={t('runtimeEvents.to')} value={draft.to} onChange={(event) => setDraft((current) => ({ ...current, to: event.currentTarget.value }))} />
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
        {rows.length === 0 ? (
          <EmptyState title={t('runtimeEvents.empty_title')} description={t('runtimeEvents.empty_description')} layout="centered" />
        ) : (
          <TableScroll label={t('runtimeEvents.title')}>
            <Table className={styles.table}>
              <Table.Thead>
                <Table.Tr>
                  <Table.Th scope="col">{t('runtimeEvents.column_time')}</Table.Th>
                  <Table.Th scope="col">{t('runtimeEvents.column_level')}</Table.Th>
                  <Table.Th scope="col">{t('runtimeEvents.column_event')}</Table.Th>
                  <Table.Th scope="col">{t('runtimeEvents.column_subject')}</Table.Th>
                  <Table.Th scope="col">{t('runtimeEvents.column_correlation')}</Table.Th>
                  <Table.Th scope="col">{t('runtimeEvents.column_source')}</Table.Th>
                  <Table.Th scope="col">{t('common.actions')}</Table.Th>
                </Table.Tr>
              </Table.Thead>
              <Table.Tbody>
                {rows.map((event) => (
                  <Table.Tr key={event.event_id}>
                    <Table.Td><time dateTime={event.occurred_at}>{formatDateTime(event.occurred_at)}</time></Table.Td>
                    <Table.Td><StatusPill tone={levelTone(event.level)}>{t(`runtimeEvents.levels.${event.level}`)}</StatusPill></Table.Td>
                    <Table.Td><span className={styles.eventCell}><strong><code>{event.event_type}</code></strong><small>{event.message}</small><StatusPill>{t(`runtimeEvents.categories.${event.category}`)}</StatusPill></span></Table.Td>
                    <Table.Td><span className={styles.stack}><code>{event.subject_type}</code><small>{event.subject_id ?? '—'}</small></span></Table.Td>
                    <Table.Td><code className={styles.breakable}>{event.correlation_id ?? '—'}</code></Table.Td>
                    <Table.Td><code>{event.source}</code></Table.Td>
                    <Table.Td><IconButton label={t('runtimeEvents.view_aria', { id: event.event_id })} onClick={() => setSelected(event)}><IconEye size={16} /></IconButton></Table.Td>
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

      {selected && <EventDetails key={selected.event_id} event={selected} onClose={() => setSelected(undefined)} />}
    </section>
  );
}
