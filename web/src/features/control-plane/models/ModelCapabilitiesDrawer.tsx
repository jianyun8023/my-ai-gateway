import { Accordion } from '@mantine/core';
import { useId, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { GATEWAY_PROTOCOLS, type EffectiveProtocolCapability, type GatewayProtocol, type LogicalModel } from '@/admin-api';
import { Button } from '@/components/ui/Button';
import { Card } from '@/components/ui/Card';
import { DetailItem, DetailList } from '@/components/ui/DetailList';
import { Modal } from '@/components/ui/Modal';
import { Notice } from '@/components/ui/Notice';
import { SegmentedTabs } from '@/components/ui/SegmentedTabs';
import { StatusPill, type StatusTone } from '@/components/ui/StatusPill';
import { PROTOCOL_LABELS, PROTOCOL_SHORT_LABELS } from '@/lib/protocols';
import { formatDateTime } from '@/utils/format';
import type { CatalogData } from './catalog';
import { modelProtocolCapabilities } from './modelCapabilities';
import { summarizeModelRouting } from './routingPresentation';
import styles from './ModelCapabilities.module.scss';

const supportTone = (mode?: string): StatusTone => mode === 'native' ? 'success'
  : mode === 'adapter' || mode === 'translated' || mode === 'degraded' ? 'warning' : 'muted';
const statusTones: Record<string, StatusTone> = {
  healthy: 'success', degraded: 'warning', unavailable: 'danger', disabled: 'muted', pending: 'warning', unknown: 'muted',
};

function CapabilityFeatures({ cell }: { cell: EffectiveProtocolCapability }) {
  const { t } = useTranslation('console');
  return <dl className={styles.features} aria-label={t('capabilities.features_title')}>
    {Object.entries(cell.effective_capabilities).map(([feature, mode]) => <div key={feature}>
      <dt>{t(`capabilities.features.${feature}`, { defaultValue: feature })}</dt>
      <dd><StatusPill tone={supportTone(mode)}>{t(`capabilities.mode.${mode}`, { defaultValue: mode })}</StatusPill></dd>
    </div>)}
  </dl>;
}

function RuntimeDiagnostics({ entry }: { entry: ReturnType<typeof modelProtocolCapabilities>['entries'][number] }) {
  const { t } = useTranslation('console');
  const { row, route, cell } = entry;
  return <Accordion order={4} keepMounted={false}>
    <Accordion.Item value="runtime">
      <Accordion.Control>{t('capabilities.diagnostics')}</Accordion.Control>
      <Accordion.Panel>
        <DetailList>
          <DetailItem label={t('capabilities.detail.route')}><code>{route.id}</code></DetailItem>
          <DetailItem label={t('capabilities.detail.source')}><code>{row.source.source_id}</code></DetailItem>
          <DetailItem label={t('capabilities.detail.account')}><code>{row.account.account_id}</code></DetailItem>
          <DetailItem label={t('capabilities.detail.binding')}><code>{cell?.binding_id ?? '—'}</code></DetailItem>
          {cell?.error && <>
            <DetailItem label={t('capabilities.cell.error_code')}><code>{cell.error.code}</code></DetailItem>
            <DetailItem label={t('capabilities.cell.error_message')}>{cell.error.message}</DetailItem>
          </>}
          {cell?.status === 'routable' && <>
            <DetailItem label={t('capabilities.detail.upstream_protocol')}>{cell.protocol_upstream ? PROTOCOL_LABELS[cell.protocol_upstream] : t('capabilities.mode.unknown')}</DetailItem>
            <DetailItem label={t('capabilities.detail.endpoint')}><code>{cell.endpoint ?? '—'}</code></DetailItem>
            <DetailItem label={t('capabilities.detail.adapter')}><code>{cell.adapter ?? t('capabilities.detail.none')}</code></DetailItem>
            <DetailItem label={t('capabilities.detail.lossy_conversion')}>{cell.allow_lossy_conversion == null ? t('capabilities.mode.unknown')
              : t(cell.allow_lossy_conversion ? 'capabilities.detail.allowed' : 'capabilities.detail.not_allowed')}</DetailItem>
            <DetailItem label={t('capabilities.detail.conversion_chain')}>
              <div className={styles.chain}>{cell.conversion_chain.length ? cell.conversion_chain.map((hop, index) => <span key={index}>
                {PROTOCOL_LABELS[hop.protocol_from]} → {PROTOCOL_LABELS[hop.protocol_to]}
                {hop.adapter && <code>{hop.adapter}</code>}
              </span>) : t('capabilities.cell.no_chain')}</div>
            </DetailItem>
          </>}
        </DetailList>
      </Accordion.Panel>
    </Accordion.Item>
  </Accordion>;
}

export function ModelCapabilitiesDrawer({ model, data, initialProtocol, open, onClose, afterExit }: {
  model: LogicalModel;
  data: CatalogData;
  initialProtocol?: GatewayProtocol;
  open: boolean;
  onClose: () => void;
  afterExit: () => void;
}) {
  const { t } = useTranslation('console');
  const id = useId();
  const [protocol, setProtocol] = useState<GatewayProtocol>(() => initialProtocol
    ?? summarizeModelRouting(model, data).protocols.find((item) => item.supported)?.protocol
    ?? data.routes.find((route) => route.logical_model_id === model.id)?.protocols[0]
    ?? GATEWAY_PROTOCOLS[0]);
  const { entries, unpublished, configured, errors, summary } = modelProtocolCapabilities(model, data, protocol);
  const selected = summary.protocols.find((item) => item.protocol === protocol)!;

  return <Modal open={open} variant="drawer" width={760} title={t('capabilities.detail.title')}
    onClose={onClose} onExitTransitionEnd={afterExit}
    footer={<Button variant="secondary" onClick={onClose}>{t('common.close')}</Button>}>
    <div className={styles.content}>
      <div className={styles.identity}>
        <strong>{model.public_name}</strong>
        {model.display_name !== model.public_name && <span>{model.display_name}</span>}
        <p>{t('capabilities.published_hint')}</p>
      </div>
      <SegmentedTabs id={id} value={protocol} onChange={setProtocol} label={t('capabilities.protocols_aria')}
        options={GATEWAY_PROTOCOLS.map((value) => ({ value, label: PROTOCOL_SHORT_LABELS[value] }))} />
      <section role="tabpanel" id={`${id}-panel`} aria-labelledby={`${id}-${protocol}`} tabIndex={0} className={styles.protocol}>
        <header className={styles.protocolHeader}>
          <h3>{PROTOCOL_LABELS[protocol]}</h3>
          <StatusPill tone={configured ? statusTones[selected.status] : 'muted'}>{t(configured ? `models.v3.${selected.status}` : 'capabilities.not_configured')}</StatusPill>
        </header>
        {!configured && <p className={styles.hint}>{t('capabilities.not_configured_hint')}</p>}
        {configured && entries.length === 0 && <p className={styles.hint}>{t('capabilities.unpublished_hint')}</p>}
        {errors.map((error) => <Notice key={JSON.stringify(error)} tone="danger">
          <strong>{t('capabilities.cell.unroutable')}</strong>
          <p>{error.message}</p><code>{error.code}</code>
          {error.route_id && <p>{t('capabilities.detail.route')}: <code>{error.route_id}</code></p>}
        </Notice>)}
        {entries.map((entry) => {
          const { row, route, cell, line } = entry;
          const role = cell?.selection === 'primary' ? t('models.v3.primary') : cell?.selection === 'fallback'
            ? t('models.v3.backup', { index: route.strategy === 'ordered_fallback' ? cell.selection_rank ?? '' : '' })
            : t('models.state.not_published');
          return <Card key={`${route.id}:${row.source.source_id}:${row.account.account_id}:${row.upstream_model_id}`} className={styles.line}>
            <header className={styles.lineHeader}>
              <div><strong>{row.source.display_name || row.source.source_id} · {row.account.display_name || row.account.account_id}</strong>
                <code>{row.upstream_model_id}</code></div>
              <div className={styles.badges}>
                <StatusPill tone={cell?.selection === 'primary' ? 'accent' : 'muted'}>{role}</StatusPill>
                {line && <StatusPill tone={statusTones[line.status]}>{t(`capabilities.line_state.${line.status}`)}</StatusPill>}
                {cell?.status === 'routable' && <StatusPill tone={supportTone(cell.mode ?? undefined)}>{t(`capabilities.mode.${cell.mode ?? 'unknown'}`)}</StatusPill>}
              </div>
            </header>
            {cell?.status === 'routable' ? <>
              {cell.degraded && <Notice tone="warning">
                <strong>{t('capabilities.mode.degraded')}</strong>
                <p>{t('capabilities.degraded_hint', { features: cell.degraded_features.map((feature) => t(`capabilities.features.${feature}`, { defaultValue: feature })).join(', ') })}</p>
              </Notice>}
              <CapabilityFeatures cell={cell} />
            </> : <Notice tone={cell ? 'danger' : 'warning'}>
              <strong>{t(cell ? 'capabilities.cell.unroutable' : 'capabilities.mode.unknown')}</strong>
              <p>{cell?.error?.message ?? t('capabilities.cell.no_snapshot_cell')}</p>
            </Notice>}
            <RuntimeDiagnostics entry={entry} />
          </Card>;
        })}
        {unpublished.map((entry) => <Card key={entry.line.id} className={styles.line}>
          <header className={styles.lineHeader}>
            <div><strong>{entry.line.sourceName} · {entry.line.accountName}</strong><code>{entry.line.upstreamModelId}</code></div>
            <StatusPill tone={statusTones[entry.status]}>{t(`capabilities.line_state.${entry.status}`)}</StatusPill>
          </header>
          <p className={styles.hint}>{t('capabilities.unpublished_line')}</p>
          {entry.line.healthStatus === 'cooling_down' && <p className={styles.hint}>
            {t('capabilities.cooling_down')}{entry.line.cooldownUntil && ` · ${formatDateTime(entry.line.cooldownUntil)}`}
          </p>}
        </Card>)}
      </section>
      <Accordion order={3} keepMounted={false}>
        <Accordion.Item value="snapshot">
          <Accordion.Control>{t('capabilities.snapshot')}</Accordion.Control>
          <Accordion.Panel><DetailList>
            <DetailItem label={t('capabilities.detail.logical_model')}><code>{model.id}</code></DetailItem>
            <DetailItem label={t('capabilities.revision')}>{data.capabilities.snapshot_revision}</DetailItem>
            <DetailItem label={t('capabilities.generated_at')}>{formatDateTime(data.capabilities.snapshot_generated_at)}</DetailItem>
            <DetailItem label={t('capabilities.fact_source')}><code>{data.capabilities.fact_source} · {data.capabilities.version}</code></DetailItem>
          </DetailList></Accordion.Panel>
        </Accordion.Item>
      </Accordion>
    </div>
  </Modal>;
}
