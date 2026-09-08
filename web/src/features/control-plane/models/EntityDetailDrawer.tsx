import { Button } from '@/components/ui/Button';
import {
  IconPencil
} from '@/components/ui/icons';
import { Modal } from '@/components/ui/Modal';
import { StatusPill } from '@/components/ui/StatusPill';
import styles from '@/features/control-plane/ControlPlane.module.scss';
import { resolvedCellsForBinding, statusTone, type CatalogData, type DetailTarget } from '@/features/control-plane/models/catalog';
import { RuntimeBindingSummary } from '@/features/control-plane/models/RuntimeBindingSummary';
import { DetailItem, DetailList, DrawerSection, EmptyTable, ProtocolPill } from '@/features/control-plane/shared';
import { PROTOCOL_LABELS } from '@/lib/protocols';
import { formatDateTime } from '@/utils/format';
import { useState } from 'react';
import { useTranslation } from 'react-i18next';

export function EntityDetailDrawer({
  target,
  data,
  onClose,
  onEdit,
}: {
  target: DetailTarget;
  data: CatalogData;
  onClose: () => void;
  onEdit: () => void;
}) {
  const { t } = useTranslation('console');
  const [open, setOpen] = useState(true);
  const [editAfterExit, setEditAfterExit] = useState(false);
  const title = target.kind === 'logical-model' ? t('models.detail.lm_title') : target.kind === 'binding' ? t('models.detail.binding_title') : t('models.detail.route_title');
  const bindingCells = target.kind === 'binding' ? resolvedCellsForBinding(data.capabilities, target.record.id) : [];
  const routeRows = target.kind === 'route' ? data.capabilities.data.filter((row) => row.route_id === target.record.id) : [];
  return (
    <Modal open={open} variant="drawer" width={600} title={title} onClose={() => setOpen(false)} onExitTransitionEnd={editAfterExit ? onEdit : onClose} footer={<><Button variant="secondary" onClick={() => setOpen(false)}>{t('common.close')}</Button><Button onClick={() => { setEditAfterExit(true); setOpen(false); }}><IconPencil size={14} />{t('common.edit')}</Button></>}>
      <DrawerSection title={t('models.detail.domain_record')}>
        {target.kind === 'logical-model' && <DetailList>
          <DetailItem label={t('models.field.lm_id')}><code>{target.record.id}</code></DetailItem>
          <DetailItem label={t('models.field.public_name')}><code>{target.record.public_name}</code></DetailItem>
          <DetailItem label={t('models.field.display_name')}>{target.record.display_name}</DetailItem>
          <DetailItem label={t('common.status')}><StatusPill tone={statusTone(target.record.status)}>{t(`values.status.${target.record.status}`, { defaultValue: target.record.status })}</StatusPill></DetailItem>
          <DetailItem label={t('models.field.enabled')}>{String(target.record.enabled)}</DetailItem>
          <DetailItem label={t('common.updated_at')}>{formatDateTime(target.record.updated_at)}</DetailItem>
        </DetailList>}
        {target.kind === 'binding' && <DetailList>
          <DetailItem label={t('models.field.binding_id')}><code>{target.record.id}</code></DetailItem>
          <DetailItem label={t('models.field.lm')}><code>{target.record.logical_model_id}</code></DetailItem>
          <DetailItem label={t('models.field.source_account')}><code>{target.record.source_id} / {target.record.account_id}</code></DetailItem>
          <DetailItem label={t('models.field.upstream_model')}><code>{target.record.upstream_model_id}</code></DetailItem>
          <DetailItem label={t('models.field.protocol')}><ProtocolPill protocol={target.record.protocol} /></DetailItem>
          <DetailItem label={t('models.field.priority')}>{target.record.priority}</DetailItem>
          <DetailItem label={t('common.status')}><StatusPill tone={statusTone(target.record.status)}>{t(`values.status.${target.record.status}`, { defaultValue: target.record.status })}</StatusPill></DetailItem>
          <DetailItem label={t('models.field.enabled')}>{String(target.record.enabled)}</DetailItem>
        </DetailList>}
        {target.kind === 'route' && <DetailList>
          <DetailItem label={t('models.field.route_id')}><code>{target.record.id}</code></DetailItem>
          <DetailItem label={t('models.field.lm')}><code>{target.record.logical_model_id}</code></DetailItem>
          <DetailItem label={t('models.field.public_name')}><code>{target.record.public_name}</code></DetailItem>
          <DetailItem label={t('models.field.protocols')}><span className={styles.inlineActions}>{target.record.protocols.map((protocol) => <ProtocolPill key={protocol} protocol={protocol} />)}</span></DetailItem>
          <DetailItem label={t('models.field.strategy')}><code>{t(`values.strategy.${target.record.strategy}`, { defaultValue: target.record.strategy })}</code></DetailItem>
          <DetailItem label={t('models.field.lossy_value')}>{target.record.allow_lossy_conversion ? t('models.state.lossy_allowed') : t('models.state.lossy_blocked')}</DetailItem>
          <DetailItem label={t('models.field.enabled')}>{String(target.record.enabled)}</DetailItem>
        </DetailList>}
      </DrawerSection>
      {target.kind === 'binding' && <DrawerSection title={t('models.detail.runtime_resolution')}><RuntimeBindingSummary cells={bindingCells} /></DrawerSection>}
      {target.kind === 'route' && <DrawerSection title={t('models.detail.published_binding')}>
        {routeRows.length === 0 ? <EmptyTable title={t('models.detail.not_in_snapshot')} /> : (
          <div className={styles.runtimeList}>{routeRows.map((row) => (
            <div key={`${row.source.source_id}:${row.account.account_id}:${row.upstream_model_id}`}>
              <span><strong>{row.source.display_name ?? row.source.source_id}</strong><small>{row.account.display_name ?? row.account.account_id} · {row.upstream_model_id}</small></span>
              <div>{row.protocols.map((cell) => <StatusPill key={cell.protocol_in} tone={cell.status === 'unroutable' ? 'muted' : cell.mode === 'native' ? 'success' : 'warning'}>{PROTOCOL_LABELS[cell.protocol_in]} · {cell.status === 'routable' ? t(`values.mode.${cell.mode}`, { defaultValue: cell.mode }) : t('models.state.unroutable')}</StatusPill>)}</div>
            </div>
          ))}</div>
        )}
      </DrawerSection>}
    </Modal>
  );
}
