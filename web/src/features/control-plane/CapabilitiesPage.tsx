import { DetailItem, DetailList } from '@/components/ui/DetailList';
import { Table } from '@mantine/core';
import { TextField, SelectField } from '@/components/ui/FormField';
import type {
  CapabilityMatrixResponse,
  CapabilityMatrixRow,
  EffectiveProtocolCapability,
  GatewayAdminResources,
  GatewayProtocol,
} from '@/admin-api';
import { GATEWAY_PROTOCOLS } from '@/admin-api';
import { Button } from '@/components/ui/Button';
import { Card } from '@/components/ui/Card';
import { IconButton } from '@/components/ui/IconButton';
import {
  IconEye,
  IconRefreshCw,
} from '@/components/ui/icons';
import { LoadingState } from '@/components/ui/LoadingState';
import { Modal } from '@/components/ui/Modal';
import { StatusPill } from '@/components/ui/StatusPill';
import { TableScroll } from '@/components/ui/TableScroll';
import { useAdminQuery } from '@/hooks/useAdminQuery';
import { PROTOCOL_LABELS } from '@/lib/protocols';
import { formatDateTime } from '@/utils/format';
import type { TFunction } from 'i18next';
import { useCallback, useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import styles from './ControlPlane.module.scss';
import { DrawerSection, EmptyTable, ErrorState, FilterBar, PageActions, ProtocolPill } from './shared';

interface CapabilitiesPageProps {
  api: GatewayAdminResources;
  refreshRevision?: number;
  onBusyChange?: (busy: boolean) => void;
}

type CapabilityFilter = 'all' | 'routable' | 'degraded' | 'unroutable';

/** 模式值在数据层保持原形(native/translated/degraded/…),仅给用户可见的标签做本地化;未知值回退原样。 */
const modeLabel = (t: TFunction, mode: string | null | undefined): string => {
  if (mode === 'native' || mode === 'translated' || mode === 'degraded') return t(`capabilities.mode.${mode}`);
  return mode ?? '';
};

const protocolCell = (
  row: CapabilityMatrixRow,
  protocol: GatewayProtocol,
): EffectiveProtocolCapability | undefined => row.protocols.find((cell) => cell.protocol_in === protocol);

const matchesStatus = (row: CapabilityMatrixRow, filter: CapabilityFilter): boolean => {
  if (filter === 'all') return true;
  if (filter === 'degraded') return row.protocols.some((cell) => cell.degraded);
  return row.protocols.some((cell) => cell.status === filter);
};

function CapabilityCell({ cell }: { cell?: EffectiveProtocolCapability }) {
  const { t } = useTranslation('console');
  if (!cell) {
    return (
      <span className={styles.capabilityCell} data-state="missing">
        <StatusPill tone="danger">{t('capabilities.cell.contract_missing')}</StatusPill>
      </span>
    );
  }
  if (cell.status === 'unroutable') {
    return (
      <span className={styles.capabilityCell} data-state="unroutable">
        <StatusPill tone="muted">{t('capabilities.cell.unroutable')}</StatusPill>
        <small>{cell.error?.code ?? 'unknown_error'}</small>
      </span>
    );
  }
  return (
    <span className={styles.capabilityCell} data-state={cell.mode}>
      <span>
        <StatusPill tone={cell.mode === 'native' ? 'success' : 'warning'}>{cell.mode ? modeLabel(t, cell.mode) : t('capabilities.cell.mode_missing')}</StatusPill>
        <StatusPill tone={cell.selection === 'primary' ? 'accent' : 'muted'}>{cell.selection ?? t('capabilities.cell.selection_missing')} #{cell.selection_rank ?? '—'}</StatusPill>
      </span>
      <small>{cell.protocol_upstream ? PROTOCOL_LABELS[cell.protocol_upstream] : t('capabilities.cell.protocol_missing')}</small>
      {cell.degraded && <StatusPill tone="warning">{t('capabilities.cell.degraded', { count: cell.degraded_features.length })}</StatusPill>}
    </span>
  );
}

function ConversionChain({ cell }: { cell: EffectiveProtocolCapability }) {
  const { t } = useTranslation('console');
  if (cell.conversion_chain.length === 0) return <span className={styles.secondaryText}>{t('capabilities.cell.no_chain')}</span>;
  return (
    <div className={styles.conversionChain}>
      {cell.conversion_chain.map((hop, index) => (
        <span key={`${hop.protocol_from}:${hop.protocol_to}:${index}`}>
          <code>{PROTOCOL_LABELS[hop.protocol_from]}</code>
          <i>→</i>
          {hop.mode === 'adapter' && <StatusPill tone="warning">{hop.adapter ?? t('capabilities.cell.adapter_missing')}</StatusPill>}
          <code>{PROTOCOL_LABELS[hop.protocol_to]}</code>
        </span>
      ))}
    </div>
  );
}

function ProtocolDetail({ cell, protocol }: { cell?: EffectiveProtocolCapability; protocol: GatewayProtocol }) {
  const { t } = useTranslation('console');
  if (!cell) {
    return (
      <section className={styles.protocolDetail} data-state="missing">
        <header><ProtocolPill protocol={protocol} /><StatusPill tone="danger">{t('capabilities.cell.contract_missing')}</StatusPill></header>
        <p>{t('capabilities.cell.no_snapshot_cell')}</p>
      </section>
    );
  }
  if (cell.status === 'unroutable') {
    return (
      <section className={styles.protocolDetail} data-state="unroutable">
        <header><ProtocolPill protocol={protocol} /><StatusPill tone="muted">{t('capabilities.cell.unroutable')}</StatusPill></header>
        <DetailList>
          <DetailItem label={t('capabilities.cell.error_code')}><code>{cell.error?.code ?? 'unknown_error'}</code></DetailItem>
          <DetailItem label={t('capabilities.cell.error_message')}>{cell.error?.message ?? t('capabilities.cell.no_error_message')}</DetailItem>
          <DetailItem label={t('capabilities.detail.route')}><code>{cell.error?.route_id ?? '—'}</code></DetailItem>
          <DetailItem label={t('capabilities.detail.mode')}>{t('capabilities.detail.mode_unpublished')}</DetailItem>
        </DetailList>
      </section>
    );
  }
  return (
    <section className={styles.protocolDetail} data-state={cell.mode}>
      <header>
        <ProtocolPill protocol={protocol} />
        <StatusPill tone={cell.mode === 'native' ? 'success' : 'warning'}>{modeLabel(t, cell.mode)}</StatusPill>
        <StatusPill tone={cell.selection === 'primary' ? 'accent' : 'muted'}>{cell.selection} #{cell.selection_rank}</StatusPill>
        {cell.degraded && <StatusPill tone="warning">{t('capabilities.mode.degraded')}</StatusPill>}
      </header>
      <DetailList>
        <DetailItem label={t('capabilities.detail.binding')}><code>{cell.binding_id}</code></DetailItem>
        <DetailItem label={t('capabilities.detail.upstream_protocol')}>{cell.protocol_upstream ? <ProtocolPill protocol={cell.protocol_upstream} /> : '—'}</DetailItem>
        <DetailItem label={t('capabilities.detail.endpoint')}><code>{cell.endpoint ?? '—'}</code></DetailItem>
        <DetailItem label={t('capabilities.detail.adapter')}><code>{cell.adapter ?? t('capabilities.detail.none')}</code></DetailItem>
        <DetailItem label={t('capabilities.detail.lossy_conversion')}>{cell.allow_lossy_conversion ? t('capabilities.detail.allowed') : t('capabilities.detail.not_allowed')}</DetailItem>
        <DetailItem label={t('capabilities.detail.conversion_chain')}><ConversionChain cell={cell} /></DetailItem>
        <DetailItem label={t('capabilities.detail.degraded_features')}>{cell.degraded_features.length > 0 ? cell.degraded_features.join(', ') : t('capabilities.detail.none')}</DetailItem>
      </DetailList>
      <div className={styles.featureMatrix}>
        {Object.entries(cell.effective_capabilities).map(([feature, mode]) => (
          <span key={feature}>
            <code>{feature}</code>
            <StatusPill tone={mode === 'native' ? 'success' : mode === 'translated' ? 'warning' : 'muted'}>{modeLabel(t, mode)}</StatusPill>
          </span>
        ))}
      </div>
    </section>
  );
}

function CapabilityDrawer({ row, onClose }: { row: CapabilityMatrixRow; onClose: () => void }) {
  const { t } = useTranslation('console');
  const [open, setOpen] = useState(true);
  return (
    <Modal open={open} variant="drawer" width={620} title={t('capabilities.detail.title')} onClose={() => setOpen(false)} onExitTransitionEnd={onClose} footer={<Button variant="secondary" onClick={() => setOpen(false)}>{t('common.close')}</Button>}>
      <DrawerSection title={t('capabilities.detail.runtime_binding')}>
        <DetailList>
          <DetailItem label={t('capabilities.detail.route')}><code>{row.route_id}</code></DetailItem>
          <DetailItem label={t('capabilities.detail.source')}><span>{row.source.display_name ?? row.source.source_id}<small className={styles.blockMeta}>{row.source.source_id}</small></span></DetailItem>
          <DetailItem label={t('capabilities.detail.account')}><span>{row.account.display_name ?? row.account.account_id}<small className={styles.blockMeta}>{row.account.account_id}</small></span></DetailItem>
          <DetailItem label={t('capabilities.detail.account_enabled')}>{row.account.enabled === null || row.account.enabled === undefined ? t('capabilities.detail.unknown') : row.account.enabled ? t('capabilities.detail.true') : t('capabilities.detail.false')}</DetailItem>
          <DetailItem label={t('capabilities.detail.logical_model')}><code>{row.model}</code></DetailItem>
          <DetailItem label={t('capabilities.detail.upstream_model')}><code>{row.upstream_model_id}</code></DetailItem>
        </DetailList>
      </DrawerSection>
      <DrawerSection title={t('capabilities.detail.fixed_protocols')}>
        <div className={styles.protocolDetailGrid}>
          {GATEWAY_PROTOCOLS.map((protocol) => <ProtocolDetail key={protocol} protocol={protocol} cell={protocolCell(row, protocol)} />)}
        </div>
      </DrawerSection>
    </Modal>
  );
}

export function CapabilitiesPage({ api, refreshRevision = 0, onBusyChange }: CapabilitiesPageProps) {
  const { t } = useTranslation('console');
  const [search, setSearch] = useState('');
  const [sourceFilter, setSourceFilter] = useState('');
  const [statusFilter, setStatusFilter] = useState<CapabilityFilter>('all');
  const [selectedRow, setSelectedRow] = useState<CapabilityMatrixRow>();
  const load = useCallback((signal: AbortSignal): Promise<CapabilityMatrixResponse> => api.capabilities(signal), [api]);
  const query = useAdminQuery({ load, refreshRevision, onBusyChange });
  const response = query.data;

  const sources = useMemo(() => [...new Set((response?.data ?? []).map((row) => row.source.source_id))].sort(), [response?.data]);
  const normalizedSearch = search.trim().toLowerCase();
  const rows = useMemo(() => (response?.data ?? []).filter((row) => {
    if (sourceFilter && row.source.source_id !== sourceFilter) return false;
    if (!matchesStatus(row, statusFilter)) return false;
    if (!normalizedSearch) return true;
    return [row.route_id, row.source.source_id, row.source.display_name, row.account.account_id, row.account.display_name, row.model, row.model_display_name, row.upstream_model_id]
      .some((value) => value?.toLowerCase().includes(normalizedSearch));
  }), [normalizedSearch, response?.data, sourceFilter, statusFilter]);

  if (query.loading && !response) return <LoadingState label={t('capabilities.loading')} />;
  if (query.error && !response) return <ErrorState error={query.error} onRetry={query.reload} />;
  if (!response) return null;

  return (
    <section className={styles.page} data-od-id="page-capabilities">
      <PageActions>
        <div className={styles.snapshotMeta}>
          <span><strong>{t('capabilities.revision', { revision: response.snapshot_revision })}</strong><small>{formatDateTime(response.snapshot_generated_at)}</small></span>
          <StatusPill tone="accent">{response.fact_source}</StatusPill>
          <StatusPill>{response.version}</StatusPill>
        </div>
        <Button variant="secondary" onClick={query.reload} loading={query.refreshing}><IconRefreshCw size={14} />{t('capabilities.refresh')}</Button>
      </PageActions>

      {query.error && <ErrorState error={query.error} onRetry={query.reload} />}

      <FilterBar label={t('capabilities.filters_aria')}>
        <TextField label={t('capabilities.search_label')} value={search} onChange={(event) => setSearch(event.target.value)} placeholder={t('capabilities.search_placeholder')} />
        <SelectField label={t('capabilities.source_filter')} value={sourceFilter} onChange={(event) => setSourceFilter(event.target.value)}><option value="">{t('capabilities.all_sources')}</option>{sources.map((source) => <option key={source} value={source}>{source}</option>)}</SelectField>
        <SelectField label={t('capabilities.route_state_filter')} value={statusFilter} onChange={(event) => setStatusFilter(event.target.value as CapabilityFilter)}><option value="all">{t('capabilities.route_state_all')}</option><option value="routable">{t('capabilities.route_state_routable')}</option><option value="degraded">{t('capabilities.route_state_degraded')}</option><option value="unroutable">{t('capabilities.route_state_unroutable')}</option></SelectField>
        <span className={styles.filterMeta}>{t('capabilities.rows_counter', { rows: rows.length, total: response.data.length })}</span>
      </FilterBar>

      {response.data.length === 0 ? (
        <EmptyTable title={t('capabilities.empty_snapshot_title')} description={t('capabilities.empty_snapshot_desc')} />
      ) : rows.length === 0 ? <EmptyTable title={t('capabilities.empty_filter')} /> : (
        <Card variant="flush" title={t('capabilities.card_title')}>
          <TableScroll label={t('capabilities.matrix_aria')}>
            <Table className={`${styles.table} ${styles.capabilitiesTable}`}>
              <Table.Thead><Table.Tr><Table.Th scope="col">{t('capabilities.column_route')}</Table.Th><Table.Th scope="col">{t('capabilities.column_source_account')}</Table.Th>{GATEWAY_PROTOCOLS.map((protocol) => <Table.Th scope="col" key={protocol}>{PROTOCOL_LABELS[protocol]}</Table.Th>)}<Table.Th scope="col">{t('common.actions')}</Table.Th></Table.Tr></Table.Thead>
              <Table.Tbody>{rows.map((row) => (
                <Table.Tr key={`${row.route_id}:${row.source.source_id}:${row.account.account_id}:${row.upstream_model_id}`}>
                  <Table.Td><span className={styles.primaryText}><strong>{row.model_display_name || row.model}</strong><small><code>{row.model}</code> · {t('capabilities.row_route_label')} <code>{row.route_id}</code></small><small>{t('capabilities.row_upstream_label')} <code>{row.upstream_model_id}</code></small></span></Table.Td>
                  <Table.Td><span className={styles.primaryText}><strong>{row.source.display_name ?? row.source.source_id}</strong><small>{row.source.source_id}</small><small>{row.account.display_name ?? row.account.account_id} · {row.account.account_id}</small></span></Table.Td>
                  {GATEWAY_PROTOCOLS.map((protocol) => <Table.Td key={protocol}><CapabilityCell cell={protocolCell(row, protocol)} /></Table.Td>)}
                  <Table.Td><IconButton label={t('capabilities.view_aria', { route: row.route_id })} onClick={() => setSelectedRow(row)}><IconEye size={16} /></IconButton></Table.Td>
                </Table.Tr>
              ))}</Table.Tbody>
            </Table>
          </TableScroll>
        </Card>
      )}

      {selectedRow && <CapabilityDrawer row={selectedRow} onClose={() => setSelectedRow(undefined)} />}
    </section>
  );
}
