import { Checkbox, Popover } from '@/components/ui/overlays';
import { Button } from '@/components/ui/Button';
import { Card } from '@/components/ui/Card';
import { EmptyState } from '@/components/ui/EmptyState';
import { StatusPill } from '@/components/ui/StatusPill';
import { EVENT_COLUMNS, EVENT_COLUMN_LABELS, type EventColumn } from '@/features/usage/eventColumns';
import { formatFallbackReason, formatTime } from '@/features/usage/formatters';
import styles from '@/features/usage/Usage.module.scss';
import { UsageBadge } from '@/features/usage/UsageBadge';
import { EventDetails } from '@/features/usage/UsageEventDetails';
import { GatewayUsageClient, type UsageEventViewModel } from '@/gateway-usage';
import { formatCompact, formatExactInteger } from '@/utils/formatCompact';
import { useVirtualizer } from '@tanstack/react-virtual';
import type { TFunction } from 'i18next';
import { useEffect, useRef, useState, type CSSProperties } from 'react';
import { useTranslation } from 'react-i18next';

const renderEventCell = (event: UsageEventViewModel, column: EventColumn, t: TFunction) => {
  switch (column) {
    case 'time': return <time>{formatTime(event.createdAt)}</time>;
    case 'logicalModel': return <strong>{event.logicalModel}</strong>;
    case 'upstreamModel': return event.upstreamModel;
    case 'provider': return event.provider;
    case 'sourceAccount': return <span>{event.sourceId}<small>{event.account}</small></span>;
    case 'clientSource': return event.clientSource;
    case 'protocol': return <span>{event.protocolIn}<small>→ {event.protocolUpstream}</small></span>;
    case 'status': return <StatusPill tone={event.success ? 'success' : 'danger'}>{event.statusCode || '—'} · {event.success ? t('usage.event.success') : t('usage.event.failure')}</StatusPill>;
    case 'retries': {
      if (!event.fallback) return String(event.retryCount);
      const label = event.retryCount > 0
        ? t('usage.event.retries_fallback', { count: event.retryCount })
        : t('usage.event.fallback_only');
      return event.fallbackReason
        ? <span title={formatFallbackReason(t, event.fallbackReason)}>{label}</span>
        : label;
    }
    case 'latency': return `${formatExactInteger(event.latencyMs)} ms`;
    case 'tokens': return formatCompact(event.tokens.total);
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
  // TanStack Virtual intentionally exposes imperative measurement helpers.
  // eslint-disable-next-line react-hooks/incompatible-library
  const virtualizer = useVirtualizer({
    count: events.length,
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
      <div className={styles.eventTable} style={{ '--event-columns': visibleColumns.length } as CSSProperties}>
        <div className={styles.eventHeader}>{visibleColumns.map((column) => <span key={column}>{t(EVENT_COLUMN_LABELS[column])}</span>)}</div>
        <div ref={parentRef} className={styles.eventScroll}>
          <div style={{ height: virtualizer.getTotalSize(), position: 'relative' }}>
            {virtualItems.map((virtualRow) => {
              const event = events[virtualRow.index];
              return (
                <button key={`${event.id}:${event.createdAt}`} className={styles.eventRow} style={{ transform: `translateY(${virtualRow.start}px)` }} onClick={() => setSelectedEvent(event)}>
                  {visibleColumns.map((column) => <span key={column}>{renderEventCell(event, column, t)}</span>)}
                </button>
              );
            })}
          </div>
          {loadingMore && <div className={styles.loadingMore}>{t('common.load_more')}</div>}
        </div>
      </div>
      {selectedEvent && <EventDetails key={selectedEvent.requestId} event={selectedEvent} onClose={() => setSelectedEvent(undefined)} client={client} />}
    </Card>
  );
}
