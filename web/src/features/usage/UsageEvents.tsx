import { Table } from '@mantine/core';
import { IconButton } from '@/components/ui/IconButton';
import { IconEye, IconInfoCircle } from '@/components/ui/icons';
import { LoadingState } from '@/components/ui/LoadingState';
import { Checkbox, Popover, Tooltip } from '@/components/ui/overlays';
import { Button } from '@/components/ui/Button';
import { Card } from '@/components/ui/Card';
import { EmptyState } from '@/components/ui/EmptyState';
import { Notice } from '@/components/ui/Notice';
import { CacheCell, TokenCell } from './EventMetricCells';
import { UsageStatus } from './UsageStatus';
import { EVENT_COLUMNS, EVENT_COLUMN_HINTS, EVENT_COLUMN_LABELS, eventTableGridTemplate, eventTableMinWidth, NUMERIC_EVENT_COLUMNS, type EventColumn } from '@/features/usage/eventColumns';
import { formatDuration, formatFallbackReason, formatTime } from '@/features/usage/formatters';
import styles from '@/features/usage/Usage.module.scss';
import { UsageBadge } from '@/features/usage/UsageBadge';
import { EventDetails } from '@/features/usage/UsageEventDetails';
import { GatewayUsageClient, type UsageEventViewModel } from '@/gateway-usage';
import { useVirtualizer } from '@tanstack/react-virtual';
import type { TFunction } from 'i18next';
import { useCallback, useEffect, useMemo, useRef, useState, type CSSProperties } from 'react';
import { useTranslation } from 'react-i18next';

// Keep the sticky header out of the virtual body coordinates.
const EVENT_HEADER_HEIGHT = 44;
const eventIdentity = (event: UsageEventViewModel) => `${event.id}:${event.createdAt}`;

const renderEventCell = (event: UsageEventViewModel, column: EventColumn, t: TFunction) => {
  switch (column) {
    case 'time': return <time>{formatTime(event.createdAt)}</time>;
    case 'logicalModel': return <strong>{event.logicalModel}</strong>;
    case 'upstreamModel': return event.upstreamModel;
    case 'provider': return event.provider;
    case 'sourceAccount': return <span>{event.sourceId}<small>{event.account}</small></span>;
    case 'clientSource': return event.clientSource;
    case 'protocol': return <span>{event.protocolIn}<small>→ {event.protocolUpstream}</small></span>;
    case 'status': return <UsageStatus success={event.success} statusCode={event.statusCode} />;
    case 'retries': {
      if (!event.fallback) return String(event.retryCount);
      const label = event.retryCount > 0
        ? t('usage.event.retries_fallback', { count: event.retryCount })
        : t('usage.event.fallback_only');
      return event.fallbackReason
        ? <span title={formatFallbackReason(t, event.fallbackReason)}>{label}</span>
        : label;
    }
    case 'latency': return <span title={formatDuration(event.latencyMs, true)}>{formatDuration(event.latencyMs)}</span>;
    case 'tokens': return <TokenCell event={event} />;
    case 'cache': return <CacheCell event={event} />;
    case 'usageSource': return <UsageBadge source={event.usageSource} />;
  }
};

const eventColumnHeader = (column: EventColumn, t: TFunction) => {
  const hintKey = EVENT_COLUMN_HINTS[column];
  if (!hintKey) return t(EVENT_COLUMN_LABELS[column]);
  return (
    <span className={styles.eventHeaderLabel}>
      {t(EVENT_COLUMN_LABELS[column])}
      <Tooltip label={t(hintKey)} events={{ hover: true, focus: true, touch: false }}>
        <IconInfoCircle size={13} />
      </Tooltip>
    </span>
  );
};

interface EventsTableProps {
  events: UsageEventViewModel[];
  hasMore: boolean;
  loadingMore: boolean;
  loadMoreError?: string;
  onLoadMore: () => void;
  onRetryLoadMore?: () => void;
  visibleColumns: EventColumn[];
  onVisibleColumnsChange: (columns: EventColumn[]) => void;
  onExport: (format: 'csv' | 'json') => void;
  exportingFormat?: 'csv' | 'json';
  exportError?: string;
  onRetryExport?: () => void;
  client: GatewayUsageClient;
}

export function EventsTable({ events, hasMore, loadingMore, loadMoreError, onLoadMore, onRetryLoadMore,
  visibleColumns, onVisibleColumnsChange, onExport, exportingFormat, exportError, onRetryExport, client }: EventsTableProps) {
  const { t } = useTranslation('console');
  const parentRef = useRef<HTMLDivElement>(null);
  const emptyStateRef = useRef<HTMLDivElement>(null);
  const moveFocusWhenRowsReturn = useRef(false);
  const [columnsOpen, setColumnsOpen] = useState(false);
  const [selectedEvent, setSelectedEvent] = useState<UsageEventViewModel>();
  const selectedEventInRows = useMemo(() => selectedEvent
    ? events.find((event) => eventIdentity(event) === eventIdentity(selectedEvent))
    : undefined, [events, selectedEvent]);
  const getItemKey = useCallback((index: number) => eventIdentity(events[index]), [events]);
  // TanStack Virtual intentionally exposes imperative measurement helpers.
  // eslint-disable-next-line react-hooks/incompatible-library
  const virtualizer = useVirtualizer<HTMLDivElement, HTMLTableRowElement>({
    count: events.length,
    getItemKey,
    scrollMargin: EVENT_HEADER_HEIGHT,
    getScrollElement: () => parentRef.current,
    estimateSize: () => 58,
    overscan: 10,
  });
  const virtualItems = virtualizer.getVirtualItems();
  const lastIndex = virtualItems.at(-1)?.index ?? -1;
  useEffect(() => {
    if (hasMore && !loadingMore && !loadMoreError && lastIndex >= events.length - 5) onLoadMore();
  }, [events.length, hasMore, lastIndex, loadMoreError, loadingMore, onLoadMore]);
  useEffect(() => {
    if (!selectedEvent || selectedEventInRows) return;
    setSelectedEvent(undefined);
    const target = parentRef.current ?? emptyStateRef.current;
    target?.focus({ preventScroll: true });
    moveFocusWhenRowsReturn.current = parentRef.current === null;
  }, [selectedEvent, selectedEventInRows]);
  useEffect(() => {
    if (events.length === 0 || !moveFocusWhenRowsReturn.current || !parentRef.current) return;
    parentRef.current.focus({ preventScroll: true });
    moveFocusWhenRowsReturn.current = false;
  }, [events.length]);

  if (events.length === 0) return <div ref={emptyStateRef} tabIndex={-1} data-od-id="events-empty-focus">
    <EmptyState title={t('usage.events.empty_title')} description={t('usage.events.empty_desc')} />
  </div>;

  return (
    <Card variant="flush" title={t('usage.events.title')} data-od-id="events-table" extra={<div className={styles.eventActions}><Popover opened={columnsOpen} onChange={setColumnsOpen} position="bottom-end" width={240} trapFocus>
        <Popover.Target><Button size="sm" variant="secondary" onClick={() => setColumnsOpen((value) => !value)}>{t('common.column_prefs')}</Button></Popover.Target>
        <Popover.Dropdown aria-label={t('common.column_prefs')} inert={!columnsOpen}>
          <div className={styles.columnMenu}>{EVENT_COLUMNS.map((column) => <Checkbox key={column} label={t(EVENT_COLUMN_LABELS[column])} checked={visibleColumns.includes(column)} onChange={() => onVisibleColumnsChange(visibleColumns.includes(column) ? visibleColumns.filter((item) => item !== column) : EVENT_COLUMNS.filter((item) => visibleColumns.includes(item) || item === column))} />)}</div>
        </Popover.Dropdown>
      </Popover><Button size="sm" variant="secondary" loading={exportingFormat === 'csv'} disabled={exportingFormat !== undefined} onClick={() => onExport('csv')}>{t('common.export_csv')}</Button><Button size="sm" variant="secondary" loading={exportingFormat === 'json'} disabled={exportingFormat !== undefined} onClick={() => onExport('json')}>{t('common.export_json')}</Button></div>}>
      {exportError && <Notice action={onRetryExport && <Button size="sm" variant="secondary" onClick={onRetryExport}>{t('common.retry')}</Button>}>{exportError}</Notice>}
      <div ref={parentRef} className={styles.eventScroll} role="region" aria-label={t('usage.events.title')} tabIndex={0}>
        <Table className={styles.eventTable} aria-label={t('usage.events.title')} aria-rowcount={hasMore ? -1 : events.length + 1}
          style={{
            '--event-grid-template': eventTableGridTemplate(visibleColumns),
            '--event-grid-min-width': `${eventTableMinWidth(visibleColumns)}px`,
            '--event-header-height': `${EVENT_HEADER_HEIGHT}px`,
          } as CSSProperties}>
          <Table.Thead className={styles.eventHeader}>
            <Table.Tr aria-rowindex={1} className={styles.eventGrid}>
              <Table.Th scope="col">{t('common.actions')}</Table.Th>
              {visibleColumns.map((column) => <Table.Th scope="col" key={column} className={NUMERIC_EVENT_COLUMNS.has(column) ? styles.numericColumn : undefined}>{eventColumnHeader(column, t)}</Table.Th>)}
            </Table.Tr>
          </Table.Thead>
          <Table.Tbody className={styles.eventBody} style={{ height: virtualizer.getTotalSize() }}>
            {virtualItems.map((virtualRow) => {
              const event = events[virtualRow.index];
              return (
                <Table.Tr key={virtualRow.key} ref={virtualizer.measureElement} data-index={virtualRow.index} aria-rowindex={virtualRow.index + 2}
                  data-clickable="true" className={`${styles.eventGrid} ${styles.eventRow}`}
                  style={{ transform: `translateY(${virtualRow.start - EVENT_HEADER_HEIGHT}px)` }} onClick={(click) => { click.currentTarget.querySelector('button')?.focus({ preventScroll: true }); setSelectedEvent(event); }}>
                  <Table.Td onClick={(click) => click.stopPropagation()}>
                    <IconButton label={t('usage.events.view_aria', { id: event.requestId })} onClick={() => setSelectedEvent(event)}><IconEye size={16} /></IconButton>
                  </Table.Td>
                  {visibleColumns.map((column) => <Table.Td key={column} className={NUMERIC_EVENT_COLUMNS.has(column) ? styles.numericColumn : undefined}>{renderEventCell(event, column, t)}</Table.Td>)}
                </Table.Tr>
              );
            })}
          </Table.Tbody>
        </Table>
        {loadMoreError ? <div className={styles.loadingMore}><Notice action={onRetryLoadMore && <Button size="sm" variant="secondary" onClick={onRetryLoadMore}>{t('common.retry')}</Button>}>{loadMoreError}</Notice></div>
          : loadingMore && <div className={styles.loadingMore}><LoadingState layout="inline" label={t('common.load_more')} /></div>}
      </div>
      {selectedEventInRows && <EventDetails key={selectedEventInRows.requestId} event={selectedEventInRows} onClose={() => setSelectedEvent(undefined)} client={client} />}
    </Card>
  );
}
