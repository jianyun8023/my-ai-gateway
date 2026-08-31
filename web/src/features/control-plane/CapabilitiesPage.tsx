import { useCallback, useEffect, useMemo, useState } from 'react';
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
import { Modal } from '@/components/ui/Modal';
import {
  IconEye,
  IconRefreshCw,
} from '@/components/ui/icons';
import {
  DetailItem,
  DetailList,
  DrawerSection,
  EmptyTable,
  ErrorState,
  FilterBar,
  IconButton,
  LoadingState,
  PROTOCOL_LABELS,
  PageActions,
  ProtocolPill,
  StatusPill,
  TableScroll,
  formatDateTime,
} from './shared';
import { useAdminQuery } from './useAdminQuery';
import styles from './ControlPlane.module.scss';

interface CapabilitiesPageProps {
  api: GatewayAdminResources;
  refreshRevision?: number;
  onBusyChange?: (busy: boolean) => void;
}

type CapabilityFilter = 'all' | 'routable' | 'degraded' | 'unroutable';

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
  if (!cell) {
    return (
      <span className={styles.capabilityCell} data-state="missing">
        <StatusPill tone="danger">contract missing</StatusPill>
      </span>
    );
  }
  if (cell.status === 'unroutable') {
    return (
      <span className={styles.capabilityCell} data-state="unroutable">
        <StatusPill tone="muted">unroutable</StatusPill>
        <small>{cell.error?.code ?? 'unknown_error'}</small>
      </span>
    );
  }
  return (
    <span className={styles.capabilityCell} data-state={cell.mode}>
      <span>
        <StatusPill tone={cell.mode === 'native' ? 'success' : 'warning'}>{cell.mode ?? 'mode missing'}</StatusPill>
        <StatusPill tone={cell.selection === 'primary' ? 'accent' : 'muted'}>{cell.selection ?? 'selection missing'} #{cell.selection_rank ?? '—'}</StatusPill>
      </span>
      <small>{cell.protocol_upstream ? PROTOCOL_LABELS[cell.protocol_upstream] : 'upstream protocol missing'}</small>
      {cell.degraded && <StatusPill tone="warning">degraded · {cell.degraded_features.length}</StatusPill>}
    </span>
  );
}

function ConversionChain({ cell }: { cell: EffectiveProtocolCapability }) {
  if (cell.conversion_chain.length === 0) return <span className={styles.secondaryText}>无已发布转换链</span>;
  return (
    <div className={styles.conversionChain}>
      {cell.conversion_chain.map((hop, index) => (
        <span key={`${hop.protocol_from}:${hop.protocol_to}:${index}`}>
          <code>{PROTOCOL_LABELS[hop.protocol_from]}</code>
          <i>→</i>
          {hop.mode === 'adapter' && <StatusPill tone="warning">{hop.adapter ?? 'adapter missing'}</StatusPill>}
          <code>{PROTOCOL_LABELS[hop.protocol_to]}</code>
        </span>
      ))}
    </div>
  );
}

function ProtocolDetail({ cell, protocol }: { cell?: EffectiveProtocolCapability; protocol: GatewayProtocol }) {
  if (!cell) {
    return (
      <section className={styles.protocolDetail} data-state="missing">
        <header><ProtocolPill protocol={protocol} /><StatusPill tone="danger">contract missing</StatusPill></header>
        <p>runtime snapshot 响应没有返回该固定协议单元。</p>
      </section>
    );
  }
  if (cell.status === 'unroutable') {
    return (
      <section className={styles.protocolDetail} data-state="unroutable">
        <header><ProtocolPill protocol={protocol} /><StatusPill tone="muted">unroutable</StatusPill></header>
        <DetailList>
          <DetailItem label="错误代码"><code>{cell.error?.code ?? 'unknown_error'}</code></DetailItem>
          <DetailItem label="错误消息">{cell.error?.message ?? '未提供结构化错误消息'}</DetailItem>
          <DetailItem label="Route"><code>{cell.error?.route_id ?? '—'}</code></DetailItem>
          <DetailItem label="Mode">未发布（不推断能力）</DetailItem>
        </DetailList>
      </section>
    );
  }
  return (
    <section className={styles.protocolDetail} data-state={cell.mode}>
      <header>
        <ProtocolPill protocol={protocol} />
        <StatusPill tone={cell.mode === 'native' ? 'success' : 'warning'}>{cell.mode}</StatusPill>
        <StatusPill tone={cell.selection === 'primary' ? 'accent' : 'muted'}>{cell.selection} #{cell.selection_rank}</StatusPill>
        {cell.degraded && <StatusPill tone="warning">degraded</StatusPill>}
      </header>
      <DetailList>
        <DetailItem label="Binding"><code>{cell.binding_id}</code></DetailItem>
        <DetailItem label="Upstream protocol">{cell.protocol_upstream ? <ProtocolPill protocol={cell.protocol_upstream} /> : '—'}</DetailItem>
        <DetailItem label="Endpoint"><code>{cell.endpoint ?? '—'}</code></DetailItem>
        <DetailItem label="Adapter"><code>{cell.adapter ?? 'none'}</code></DetailItem>
        <DetailItem label="Lossy conversion">{cell.allow_lossy_conversion ? 'allowed' : 'not allowed'}</DetailItem>
        <DetailItem label="Conversion chain"><ConversionChain cell={cell} /></DetailItem>
        <DetailItem label="Degraded features">{cell.degraded_features.length > 0 ? cell.degraded_features.join(', ') : 'none'}</DetailItem>
      </DetailList>
      <div className={styles.featureMatrix}>
        {Object.entries(cell.effective_capabilities).map(([feature, mode]) => (
          <span key={feature}>
            <code>{feature}</code>
            <StatusPill tone={mode === 'native' ? 'success' : mode === 'translated' ? 'warning' : 'muted'}>{mode}</StatusPill>
          </span>
        ))}
      </div>
    </section>
  );
}

function CapabilityDrawer({ row, onClose }: { row: CapabilityMatrixRow; onClose: () => void }) {
  const [open, setOpen] = useState(true);
  useEffect(() => {
    if (open) return;
    const timer = window.setTimeout(onClose, 380);
    return () => window.clearTimeout(timer);
  }, [onClose, open]);
  return (
    <Modal open={open} variant="drawer" width={620} title="有效能力详情" onClose={() => setOpen(false)} footer={<Button variant="secondary" onClick={() => setOpen(false)}>关闭</Button>}>
      <DrawerSection title="Runtime binding">
        <DetailList>
          <DetailItem label="Route"><code>{row.route_id}</code></DetailItem>
          <DetailItem label="Source"><span>{row.source.display_name ?? row.source.source_id}<small className={styles.blockMeta}>{row.source.source_id}</small></span></DetailItem>
          <DetailItem label="Account"><span>{row.account.display_name ?? row.account.account_id}<small className={styles.blockMeta}>{row.account.account_id}</small></span></DetailItem>
          <DetailItem label="Account enabled">{row.account.enabled === null || row.account.enabled === undefined ? 'unknown' : row.account.enabled ? 'true' : 'false'}</DetailItem>
          <DetailItem label="Logical model"><code>{row.model}</code></DetailItem>
          <DetailItem label="Upstream model"><code>{row.upstream_model_id}</code></DetailItem>
        </DetailList>
      </DrawerSection>
      <DrawerSection title="固定三协议">
        <div className={styles.protocolDetailGrid}>
          {GATEWAY_PROTOCOLS.map((protocol) => <ProtocolDetail key={protocol} protocol={protocol} cell={protocolCell(row, protocol)} />)}
        </div>
      </DrawerSection>
    </Modal>
  );
}

export function CapabilitiesPage({ api, refreshRevision = 0, onBusyChange }: CapabilitiesPageProps) {
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

  if (query.loading && !response) return <LoadingState label="正在读取 runtime capability snapshot…" />;
  if (query.error && !response) return <ErrorState error={query.error} onRetry={query.reload} />;
  if (!response) return null;

  return (
    <section className={styles.page} data-od-id="page-capabilities">
      <PageActions>
        <div className={styles.snapshotMeta}>
          <span><strong>revision {response.snapshot_revision}</strong><small>{formatDateTime(response.snapshot_generated_at)}</small></span>
          <StatusPill tone="accent">{response.fact_source}</StatusPill>
          <StatusPill>{response.version}</StatusPill>
        </div>
        <Button variant="secondary" onClick={query.reload} loading={query.refreshing}><IconRefreshCw size={14} />刷新快照</Button>
      </PageActions>

      {query.error && <ErrorState error={query.error} onRetry={query.reload} />}

      <FilterBar>
        <label>搜索<input value={search} onChange={(event) => setSearch(event.target.value)} placeholder="Route、Source、Account 或模型" /></label>
        <label>Source<select value={sourceFilter} onChange={(event) => setSourceFilter(event.target.value)}><option value="">全部</option>{sources.map((source) => <option key={source} value={source}>{source}</option>)}</select></label>
        <label>路由状态<select value={statusFilter} onChange={(event) => setStatusFilter(event.target.value as CapabilityFilter)}><option value="all">全部</option><option value="routable">含 routable</option><option value="degraded">含 degraded</option><option value="unroutable">含 unroutable</option></select></label>
        <span className={styles.filterMeta}>{rows.length} / {response.data.length} rows</span>
      </FilterBar>

      {response.data.length === 0 ? (
        <EmptyTable title="runtime snapshot 尚无已发布能力行" description="只有已确认、可用且已绑定到 Route 的模型会进入此矩阵。" />
      ) : rows.length === 0 ? <EmptyTable title="当前筛选没有能力行" /> : (
        <Card variant="flush" title="Effective Capabilities" subtitle="与 proxy、route resolution 和 /v1/models 共用不可变 runtime snapshot">
          <TableScroll label="有效能力矩阵">
            <table className={`${styles.table} ${styles.capabilitiesTable}`}>
              <thead><tr><th>Route / Models</th><th>Source / Account</th>{GATEWAY_PROTOCOLS.map((protocol) => <th key={protocol}>{PROTOCOL_LABELS[protocol]}</th>)}<th>操作</th></tr></thead>
              <tbody>{rows.map((row) => (
                <tr key={`${row.route_id}:${row.source.source_id}:${row.account.account_id}:${row.upstream_model_id}`}>
                  <td><span className={styles.primaryText}><strong>{row.model_display_name || row.model}</strong><small><code>{row.model}</code> · route <code>{row.route_id}</code></small><small>upstream <code>{row.upstream_model_id}</code></small></span></td>
                  <td><span className={styles.primaryText}><strong>{row.source.display_name ?? row.source.source_id}</strong><small>{row.source.source_id}</small><small>{row.account.display_name ?? row.account.account_id} · {row.account.account_id}</small></span></td>
                  {GATEWAY_PROTOCOLS.map((protocol) => <td key={protocol}><CapabilityCell cell={protocolCell(row, protocol)} /></td>)}
                  <td><IconButton label={`查看 ${row.route_id} 能力详情`} onClick={() => setSelectedRow(row)}><IconEye size={16} /></IconButton></td>
                </tr>
              ))}</tbody>
            </table>
          </TableScroll>
        </Card>
      )}

      {selectedRow && <CapabilityDrawer row={selectedRow} onClose={() => setSelectedRow(undefined)} />}
    </section>
  );
}
