import { FormActions } from '@/components/ui/FormActions';
import { DetailItem, DetailList } from '@/components/ui/DetailList';
import { clearOperationNotification, notifySuccess } from '@/components/ui/notifications';
import { Table } from '@mantine/core';
import { useOverlayState } from '@/components/ui/useOverlayState';
import { downloadBlob } from '@/utils/download';
import type {
  AdminErrorShape,
  CapabilityMatrixResponse,
  GatewayAdminResources,
  RuntimeReloadResult,
  VirtualKey,
  VirtualKeyRotateInput,
} from '@/admin-api';
import { normalizeAdminError } from '@/admin-api';
import { Button } from '@/components/ui/Button';
import { Card } from '@/components/ui/Card';
import { IconButton } from '@/components/ui/IconButton';
import {
  IconCopy,
  IconDatabase,
  IconDownload,
  IconEye,
  IconKey,
  IconPlus,
  IconRefreshCw,
  IconShield,
  IconTrash2,
} from '@/components/ui/icons';
import { LoadingState } from '@/components/ui/LoadingState';
import { Modal } from '@/components/ui/Modal';
import { StatusPill } from '@/components/ui/StatusPill';
import { TableScroll } from '@/components/ui/TableScroll';
import styles from '@/features/control-plane/ControlPlane.module.scss';
import { ConfirmDialog, EmptyTable, ErrorState, FormError, PageActions } from '@/features/control-plane/shared';
import { VirtualKeyForm } from '@/features/control-plane/VirtualKeyForm';
import { VirtualKeyRotationForm } from '@/features/control-plane/VirtualKeyRotationForm';
import { useAdminQuery } from '@/hooks/useAdminQuery';
import { useLocalizedApiError } from '@/hooks/useLocalizedApiError';
import { formatDateTime } from '@/utils/format';
import { useCallback, useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';

interface SettingsPageProps {
  api: GatewayAdminResources;
  refreshRevision?: number;
  onBusyChange?: (busy: boolean) => void;
  adminKeyConfigured: boolean;
  onClearAdminKey: () => void;
}

interface SettingsData {
  keys: VirtualKey[];
  capabilities: CapabilityMatrixResponse;
}

interface RevealedKeyResult {
  id: number;
  name: string;
  key: string;
  message?: string;
}

export function SettingsPage({
  api,
  refreshRevision = 0,
  onBusyChange,
  adminKeyConfigured,
  onClearAdminKey,
}: SettingsPageProps) {
  const { t } = useTranslation('console');
  const localizedApiError = useLocalizedApiError();
  const [createOpen, setCreateOpen] = useState(false);
  const [revealedKey, setRevealedKey] = useState<RevealedKeyResult>();
  const pendingRevealedKey = useRef<RevealedKeyResult | undefined>(undefined);
  const [copyStatus, setCopyStatus] = useState<'copied' | 'pending' | 'failed'>('pending');
  const { value: revokeTarget, setValue: setRevokeTarget, opened: revokeTargetOpen, afterExit: revokeTargetAfterExit } = useOverlayState<VirtualKey>();
  const { value: rotateTarget, setValue: setRotateTarget, opened: rotateTargetOpen, afterExit: rotateTargetAfterExit } = useOverlayState<VirtualKey>();
  const [now, setNow] = useState(Date.now);
  const [mutationBusy, setMutationBusy] = useState(false);
  const [mutationError, setMutationError] = useState<AdminErrorShape>();
  const [reloadResult, setReloadResult] = useState<RuntimeReloadResult>();

  const revealPendingKey = () => {
    const result = pendingRevealedKey.current;
    pendingRevealedKey.current = undefined;
    if (result) setRevealedKey(result);
  };

  const load = useCallback(async (signal: AbortSignal): Promise<SettingsData> => {
    const [keys, capabilities] = await Promise.all([
      api.virtualKeys(signal),
      api.capabilities(signal),
    ]);
    return { keys, capabilities };
  }, [api]);
  const query = useAdminQuery({ load, refreshRevision, onBusyChange });
  const data = query.data;

  useEffect(() => {
    if (!data?.keys.some((key) => key.expires_at || key.overlap_until)) return;
    const timer = window.setInterval(() => setNow(Date.now()), 1000);
    return () => window.clearInterval(timer);
  }, [data]);

  const mutate = async (operation: () => Promise<void>, successMessage?: string) => {
    if (mutationBusy) return;
    clearOperationNotification();
    setMutationBusy(true);
    setMutationError(undefined);
    onBusyChange?.(true);
    try {
      await operation();
      if (successMessage) notifySuccess(successMessage);
    } catch (error) {
      setMutationError(normalizeAdminError(error));
    } finally {
      setMutationBusy(false);
      onBusyChange?.(false);
    }
  };

  const copyRevealedKey = async () => {
    if (!revealedKey) return;
    try {
      await navigator.clipboard.writeText(revealedKey.key);
      setCopyStatus('copied');
    } catch {
      setCopyStatus('failed');
    }
  };

  const createKey = (name: string, allowedModels: string[]) => {
    void mutate(async () => {
      const result = await api.createVirtualKey({ name, allowed_models: allowedModels });
      pendingRevealedKey.current = { id: result.id, name: result.name, key: result.key, message: t('settings.key_created') };
      setCreateOpen(false);
      setCopyStatus('pending');
      try {
        await navigator.clipboard.writeText(result.key);
        setCopyStatus('copied');
      } catch {
        setCopyStatus('failed');
      }
      query.reload();
    });
  };

  const revealKey = (key: VirtualKey) => {
    void mutate(async () => {
      const result = await api.revealVirtualKey(key.id);
      setRevealedKey({ id: key.id, name: key.name, key: result.key });
      setCopyStatus('pending');
    });
  };

  const rotateKey = (input: VirtualKeyRotateInput) => {
    if (!rotateTarget) return;
    void mutate(async () => {
      const result = await api.rotateVirtualKey(rotateTarget.id, input);
      // An earlier expiry still applies to the old key during the overlap.
      const validUntil = result.overlap_until && rotateTarget.expires_at
        ? new Date(Math.min(Date.parse(result.overlap_until), Date.parse(rotateTarget.expires_at))).toISOString()
        : result.overlap_until;
      pendingRevealedKey.current = { id: result.new_id, name: rotateTarget.name, key: result.key,
        message: validUntil
          ? t('settings.key_rotated_overlap', { name: rotateTarget.name, until: formatDateTime(validUntil) })
          : t('settings.key_rotated_immediate', { name: rotateTarget.name }),
      };
      setCopyStatus('pending');
      setRotateTarget(undefined);
      query.reload();
    });
  };

  const revokeKey = () => {
    if (!revokeTarget) return;
    void mutate(async () => {
      await api.revokeVirtualKey(revokeTarget.id);
      setRevokeTarget(undefined);
      query.reload();
    }, t('settings.key_revoked', { name: revokeTarget.name }));
  };

  const reloadRuntime = () => {
    void mutate(async () => {
      const result = await api.reloadRuntime();
      setReloadResult(result);
      query.reload();
    }, t('settings.runtime_reloaded'));
  };

  const exportConfiguration = () => {
    void mutate(async () => {
      const exported = await api.sanitizedConfigurationExport();
      const blob = new Blob([`${JSON.stringify(exported, null, 2)}\n`], { type: 'application/json' });
      downloadBlob(blob, `my-ai-gateway-config-${new Date().toISOString().replace(/[:.]/g, '-')}.json`);
    }, t('settings.export_done'));
  };

  if (query.loading && !data) return <LoadingState label={t('settings.loading')} />;
  if (query.error && !data) return <ErrorState error={query.error} onRetry={query.reload} />;
  if (!data) return null;

  const snapshotRevision = reloadResult?.snapshot_revision ?? data.capabilities.snapshot_revision;
  const snapshotGeneratedAt = reloadResult?.snapshot_generated_at ?? data.capabilities.snapshot_generated_at;

  return (
    <section className={styles.page} data-od-id="page-settings">
      <PageActions>
        <div className={styles.snapshotMeta}>
          <span><strong>{t('settings.resources_card')}</strong></span>
          <StatusPill tone="accent">{t('settings.runtime_revision', { revision: snapshotRevision })}</StatusPill>
        </div>
        <Button variant="secondary" onClick={query.reload} loading={query.refreshing}><IconRefreshCw size={14} />{t('common.refresh')}</Button>
      </PageActions>
      {query.error && <ErrorState error={query.error} onRetry={query.reload} />}
      {mutationError && !createOpen && !revokeTarget && !rotateTarget && <ErrorState error={mutationError} />}

      <div className={styles.settingsGrid}>
        <Card title={t('settings.card.key_session')} subtitle={t('settings.card.key_session_subtitle')}>
          <div className={styles.settingsStatus}>
            <IconShield size={20} />
            <span><strong>{adminKeyConfigured ? t('settings.card.key_configured') : t('settings.card.key_not_configured')}</strong>{!adminKeyConfigured && <small>{t('settings.card.key_hint')}</small>}</span>
          </div>
          <div className={styles.cardActions}>
            <Button variant="secondary" onClick={() => { onClearAdminKey(); notifySuccess(t('settings.key_cleared')); }} disabled={!adminKeyConfigured}>{t('settings.clear_key')}</Button>
          </div>
        </Card>

        <Card title={t('settings.card.runtime')}>
          <DetailList>
            <DetailItem label={t('settings.snapshot_field.revision')}><code>{snapshotRevision}</code></DetailItem>
            <DetailItem label={t('settings.snapshot_field.generated_at')}>{formatDateTime(snapshotGeneratedAt)}</DetailItem>
            <DetailItem label={t('settings.snapshot_field.fact_source')}><StatusPill tone="accent">{data.capabilities.fact_source}</StatusPill></DetailItem>
            <DetailItem label={t('settings.snapshot_field.published_rows')}>{data.capabilities.data.length}</DetailItem>
          </DetailList>
          <div className={styles.cardActions}>
            <Button variant="secondary" onClick={reloadRuntime} loading={mutationBusy}><IconDatabase size={14} />{t('settings.reload_runtime')}</Button>
          </div>
        </Card>

        <Card title={t('settings.card.export')}>
          <div className={styles.settingsStatus}>
            <IconDownload size={20} />
            <span><strong>{t('settings.card.export_redacted')}</strong><small>{t('settings.card.export_redacted_hint')}</small></span>
          </div>
          <div className={styles.cardActions}>
            <Button variant="secondary" onClick={exportConfiguration} loading={mutationBusy}><IconDownload size={14} />{t('settings.export_json')}</Button>
          </div>
        </Card>
      </div>

      <Card variant="flush" title={t('settings.card.keys')} extra={<Button size="sm" variant="primary" onClick={() => setCreateOpen(true)}><IconPlus size={14} />{t('settings.new_key')}</Button>}>
        {data.keys.length === 0 ? <EmptyTable title={t('settings.keys_empty')} /> : (
          <TableScroll label={t('settings.keys_table_aria')}>
            <Table className={styles.table}>
              <Table.Thead><Table.Tr><Table.Th scope="col">{t('settings.keys_column.name')}</Table.Th><Table.Th scope="col">{t('settings.keys_column.prefix')}</Table.Th><Table.Th scope="col">{t('settings.keys_column.allowed_models')}</Table.Th><Table.Th scope="col">{t('settings.keys_column.created')}</Table.Th><Table.Th scope="col">{t('settings.keys_column.last_used')}</Table.Th><Table.Th scope="col">{t('settings.keys_column.status')}</Table.Th><Table.Th scope="col">{t('common.actions')}</Table.Th></Table.Tr></Table.Thead>
              <Table.Tbody>{data.keys.map((key) => {
                const expired = Boolean(key.expires_at && Date.parse(key.expires_at) <= now);
                const replaced = key.replaced_by_id != null;
                const overlapActive = replaced && Boolean(key.overlap_until && Date.parse(key.overlap_until) > now);
                const status = key.revoked_at ? 'revoked' : !key.enabled ? 'disabled' : expired ? 'expired' : replaced ? overlapActive ? 'overlap' : 'rotated' : 'active';
                const validUntil = key.overlap_until && key.expires_at
                  ? new Date(Math.min(Date.parse(key.overlap_until), Date.parse(key.expires_at))).toISOString()
                  : key.overlap_until;
                return (
                <Table.Tr key={key.id}>
                  <Table.Td><span className={styles.primaryText}><strong>{key.name}</strong><small>{t('settings.row_id', { id: key.id })}</small></span></Table.Td>
                  <Table.Td><code>{key.key_prefix}…</code></Table.Td>
                  <Table.Td>{key.allowed_models.length === 0 ? <StatusPill>{t('settings.all_models')}</StatusPill> : <span className={styles.inlineActions}>{key.allowed_models.map((model) => <StatusPill key={model}>{model}</StatusPill>)}</span>}</Table.Td>
                  <Table.Td>{formatDateTime(key.created_at)}</Table.Td>
                  <Table.Td>{formatDateTime(key.last_used_at)}</Table.Td>
                  <Table.Td><span className={styles.primaryText}><StatusPill tone={status === 'active' ? 'success' : status === 'overlap' ? 'warning' : 'muted'}>{t(`settings.key_status.${status}`)}</StatusPill>{overlapActive && status === 'overlap' && <small>{t('settings.valid_until', { until: formatDateTime(validUntil) })}</small>}</span></Table.Td>
                  <Table.Td><span className={styles.inlineActions}>
                    <IconButton label={key.key_recoverable ? t('settings.view_key_aria', { name: key.name }) : t('settings.view_key_unavailable_aria', { name: key.name })} disabled={mutationBusy || !key.key_recoverable} onClick={() => revealKey(key)}><IconEye size={16} /></IconButton>
                    <IconButton label={t('settings.rotate_key_aria', { name: key.name })} disabled={mutationBusy || status !== 'active'} onClick={() => { setMutationError(undefined); setRotateTarget(key); }}><IconRefreshCw size={16} /></IconButton>
                    <IconButton label={t('settings.revoke_key_aria', { name: key.name })} className={styles.dangerIcon} disabled={!key.enabled || Boolean(key.revoked_at)} onClick={() => setRevokeTarget(key)}><IconTrash2 size={16} /></IconButton>
                  </span></Table.Td>
                </Table.Tr>
              ); })}</Table.Tbody>
            </Table>
          </TableScroll>
        )}
      </Card>

      <Modal
        open={createOpen}
        onExitTransitionEnd={revealPendingKey}
        title={t('settings.modal.new_key')}
        width={560}
        onClose={() => !mutationBusy && setCreateOpen(false)}
        closeDisabled={mutationBusy}
        footer={<FormActions form="virtual-key-editor-form" cancelLabel={t('common.cancel')} submitLabel={t('settings.modal.create_key')} submitIcon={<IconKey size={14} />} busy={mutationBusy} onCancel={() => setCreateOpen(false)} />}
      >
        <VirtualKeyForm busy={mutationBusy} error={mutationError ? localizedApiError(mutationError) : undefined} onSubmit={createKey} />
      </Modal>

      <Modal
        open={rotateTargetOpen}
        onExitTransitionEnd={() => {
          rotateTargetAfterExit();
          revealPendingKey();
        }}
        title={t('settings.modal.rotate_title', { name: rotateTarget?.name })}
        width={560}
        onClose={() => !mutationBusy && setRotateTarget(undefined)}
        closeDisabled={mutationBusy}
        footer={<FormActions form="virtual-key-rotation-form" cancelLabel={t('common.cancel')} submitLabel={t('settings.modal.rotate_confirm')} submitIcon={<IconRefreshCw size={14} />} busy={mutationBusy} onCancel={() => setRotateTarget(undefined)} />}
      >
        {rotateTarget && <VirtualKeyRotationForm key={rotateTarget.id} target={rotateTarget} busy={mutationBusy} error={mutationError ? localizedApiError(mutationError) : undefined} onSubmit={rotateKey} />}
      </Modal>

      <Modal
        open={Boolean(revealedKey)}
        title={revealedKey ? t('settings.modal.reveal_title', { name: revealedKey.name }) : ''}
        width={520}
        onClose={() => { setRevealedKey(undefined); setCopyStatus('pending'); }}
        footer={<Button variant="secondary" onClick={() => { setRevealedKey(undefined); setCopyStatus('pending'); }}>{t('common.done')}</Button>}
      >
        {revealedKey?.message && <p>{revealedKey.message}</p>}
        <div className={styles.secretResult}>
          <IconKey size={22} />
          <div>
            <strong role="status">{copyStatus === 'copied' ? t('settings.api_key_copied') : copyStatus === 'failed' ? t('settings.api_key_copy_denied') : t('settings.modal.reveal_subtitle')}</strong>
            {revealedKey && <code className={styles.secretValue}>{revealedKey.key}</code>}
          </div>
          <Button variant="primary" onClick={() => void copyRevealedKey()}><IconCopy size={14} />{t('common.copy_api_key')}</Button>
        </div>
      </Modal>

      <ConfirmDialog
        open={revokeTargetOpen}
        onExitTransitionEnd={revokeTargetAfterExit}
        title={t('settings.modal.revoke_title')}
        description={revokeTarget ? <div className={styles.page}>{t('settings.modal.revoke_body', { name: revokeTarget.name })}<FormError message={mutationError ? localizedApiError(mutationError) : undefined} /></div> : null}
        confirmLabel={t('settings.modal.revoke_confirm')}
        danger
        busy={mutationBusy}
        onCancel={() => !mutationBusy && setRevokeTarget(undefined)}
        onConfirm={revokeKey}
      />
    </section>
  );
}
