import { Table } from '@mantine/core';
import { IconButton } from '@/components/ui/IconButton';
import { IconEye } from '@/components/ui/icons';
import { LoadingState } from '@/components/ui/LoadingState';
import { Checkbox, Popover } from '@/components/ui/overlays';
import { Button } from '@/components/ui/Button';
import { Card } from '@/components/ui/Card';
import { EmptyState } from '@/components/ui/EmptyState';
import { UsageStatus } from './UsageStatus';
import { isUnreportedUsage } from './usageQuality';
import { EVENT_COLUMNS, EVENT_COLUMN_LABELS, type EventColumn } from '@/features/usage/eventColumns';
import { formatDuration, formatFallbackReason, formatTime, formatUsageTokens } from '@/features/usage/formatters';
import styles from '@/features/usage/Usage.module.scss';
import { UsageBadge } from '@/features/usage/UsageBadge';
import { EventDetails } from '@/features/usage/UsageEventDetails';
import { GatewayUsageClient, type UsageEventViewModel } from '@/gateway-usage';
import { useVirtualizer } from '@tanstack/react-virtual';
import type { TFunction } from 'i18next';
import { useCallback, useEffect, useRef, useState, type CSSProperties } from 'react';
import { useTranslation } from 'react-i18next';

// Keep the sticky header out of the virtual body coordinates.
const EVENT_HEADER_HEIGHT = 44;

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
    case 'tokens': return event.tokens.total === 0 && isUnreportedUsage(event.usageSource) ? <UsageBadge source={event.usageSource} /> : <span title={formatUsageTokens(event.tokens.total, event.usageSource, true)}>{formatUsageTokens(event.tokens.total, event.usageSource)}</span>;
    case 'usageSource': return <UsageBadge source={event.usageSource} />;
  }
};

interface EventsTableProps {
  events: UsageEventViewModel[];
  hasMore: boolean;
  loadingMore: boolean;
  onLoadMore: () => void;
  visibleColumns: EventColumn[];
  onVisibleColumnsChange: (columns: EventColumn[]) => void;
  onExport: (format: 'csv' | 'json') => void;
  client: GatewayUsageClient;
}

export function EventsTable({ events, hasMore, loadingMore, onLoadMore, visibleColumns, onVisibleColumnsChange, onExport, client }: EventsTableProps) {
  const { t } = useTranslation('console');
  const parentRef = useRef<HTMLDivElement>(null);
  const [columnsOpen, setColumnsOpen] = useState(false);
  const [selectedEvent, setSelectedEvent] = useState<UsageEventViewModel>();
  const getItemKey = useCallback((index: number) => `${events[index].id}:${events[index].createdAt}`, [events]);
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
    if (hasMore && !loadingMore && lastIndex >= events.length - 5) onLoadMore();
  }, [events.length, hasMore, lastIndex, loadingMore, onLoadMore]);

  if (events.length === 0) return <EmptyState title={t('usage.events.empty_title')} description={t('usage.events.empty_desc')} />;

  return (
    <Card variant="flush" title={t('usage.events.title')} data-od-id="events-table" extra={<div className={styles.eventActions}><Popover opened={columnsOpen} onChange={setColumnsOpen} position="bottom-end" width={240} trapFocus>
        <Popover.Target><Button size="sm" variant="secondary" onClick={() => setColumnsOpen((value) => !value)}>{t('common.column_prefs')}</Button></Popover.Target>
        <Popover.Dropdown aria-label={t('common.column_prefs')} inert={!columnsOpen}>
          <div className={styles.columnMenu}>{EVENT_COLUMNS.map((column) => <Checkbox key={column} label={t(EVENT_COLUMN_LABELS[column])} checked={visibleColumns.includes(column)} onChange={() => onVisibleColumnsChange(visibleColumns.includes(column) ? visibleColumns.filter((item) => item !== column) : EVENT_COLUMNS.filter((item) => visibleColumns.includes(item) || item === column))} />)}</div>
        </Popover.Dropdown>
      </Popover><Button size="sm" variant="secondary" onClick={() => onExport('csv')}>{t('common.export_csv')}</Button><Button size="sm" variant="secondary" onClick={() => onExport('json')}>{t('common.export_json')}</Button></div>}>
      <div ref={parentRef} className={styles.eventScroll} role="region" aria-label={t('usage.events.title')} tabIndex={0}>
        <Table className={styles.eventTable} aria-label={t('usage.events.title')} aria-rowcount={hasMore ? -1 : events.length + 1}
          style={{ '--event-columns': visibleColumns.length, '--event-header-height': `${EVENT_HEADER_HEIGHT}px` } as CSSProperties}>
          <Table.Thead className={styles.eventHeader}>
            <Table.Tr aria-rowindex={1} className={styles.eventGrid}>
              <Table.Th scope="col">{t('common.actions')}</Table.Th>
              {visibleColumns.map((column) => <Table.Th scope="col" key={column}>{t(EVENT_COLUMN_LABELS[column])}</Table.Th>)}
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
                  {visibleColumns.map((column) => <Table.Td key={column}>{renderEventCell(event, column, t)}</Table.Td>)}
                </Table.Tr>
              );
            })}
          </Table.Tbody>
        </Table>
        {loadingMore && <div className={styles.loadingMore}><LoadingState layout="inline" label={t('common.load_more')} /></div>}
      </div>
      {selectedEvent && <EventDetails key={selectedEvent.requestId} event={selectedEvent} onClose={() => setSelectedEvent(undefined)} client={client} />}
    </Card>
  );
}
