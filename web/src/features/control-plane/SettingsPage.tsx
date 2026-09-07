import { IconButton } from '@/components/ui/IconButton';
import { LoadingState } from '@/components/ui/LoadingState';
import { StatusPill } from '@/components/ui/StatusPill';
import { TableScroll } from '@/components/ui/TableScroll';
import { TextAreaField, TextField } from '@/components/ui/FormField';
import { Button } from '@/components/ui/Button';
import { Card } from '@/components/ui/Card';
import { Modal } from '@/components/ui/Modal';
import { useCallback, useState, type FormEvent } from 'react';
import { useTranslation } from 'react-i18next';
import type {
  AdminErrorShape,
  CapabilityMatrixResponse,
  GatewayAdminResources,
  RuntimeReloadResult,
  VirtualKey,
} from '@/admin-api';
import { normalizeAdminError } from '@/admin-api';
import { useLocalizedApiError } from '@/hooks/useLocalizedApiError';
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
import {
  ConfirmDialog,
  DetailItem,
  DetailList,
  EmptyTable,
  ErrorState,
  FormError,
  FormGrid,
  PageActions,
  SuccessNotice,
  formatDateTime,
} from './shared';
import { useAdminQuery } from './useAdminQuery';
import styles from './ControlPlane.module.scss';

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

function VirtualKeyForm({
  busy,
  error,
  onSubmit,
}: {
  busy: boolean;
  error?: string;
  onSubmit: (name: string, allowedModels: string[]) => void;
}) {
  const { t } = useTranslation('console');
  const [name, setName] = useState('');
  const [allowedModels, setAllowedModels] = useState('');
  const [validationError, setValidationError] = useState('');

  const submit = (event: FormEvent) => {
    event.preventDefault();
    if (!name.trim()) {
      setValidationError(t('settings.key_name_required'));
      return;
    }
    setValidationError('');
    onSubmit(
      name.trim(),
      [...new Set(allowedModels.split(',').map((model) => model.trim()).filter(Boolean))],
    );
  };

  return (
    <form id="virtual-key-editor-form" className={styles.page} onSubmit={submit}>
      <FormGrid>
        <TextField label={t('settings.key_name')} value={name} disabled={busy} onChange={(event) => setName(event.target.value)} autoComplete="off" />
        <TextAreaField label={t('settings.allowed_models')} hint={t('settings.allowed_models_hint')} value={allowedModels} disabled={busy} onChange={(event) => setAllowedModels(event.target.value)} />
      </FormGrid>
      <FormError message={validationError || error} />
    </form>
  );
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
  const [revealedKey, setRevealedKey] = useState<{ id: number; name: string; key: string }>();
  const [copyStatus, setCopyStatus] = useState<'copied' | 'pending' | 'failed'>('pending');
  const [revokeTarget, setRevokeTarget] = useState<VirtualKey>();
  const [mutationBusy, setMutationBusy] = useState(false);
  const [mutationError, setMutationError] = useState<AdminErrorShape>();
  const [notice, setNotice] = useState('');
  const [reloadResult, setReloadResult] = useState<RuntimeReloadResult>();

  const load = useCallback(async (signal: AbortSignal): Promise<SettingsData> => {
    const [keys, capabilities] = await Promise.all([
      api.virtualKeys(signal),
      api.capabilities(signal),
    ]);
    return { keys, capabilities };
  }, [api]);
  const query = useAdminQuery({ load, refreshRevision, onBusyChange });
  const data = query.data;

  const mutate = async (operation: () => Promise<void>, successMessage?: string) => {
    if (mutationBusy) return;
    setMutationBusy(true);
    setMutationError(undefined);
    onBusyChange?.(true);
    try {
      await operation();
      if (successMessage) setNotice(successMessage);
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
      setCreateOpen(false);
      setRevealedKey({ id: result.id, name: result.name, key: result.key });
      setCopyStatus('pending');
      try {
        await navigator.clipboard.writeText(result.key);
        setCopyStatus('copied');
      } catch {
        setCopyStatus('failed');
      }
      setNotice(t('settings.key_created'));
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
      const url = URL.createObjectURL(blob);
      const anchor = document.createElement('a');
      anchor.href = url;
      anchor.download = `my-ai-gateway-config-${new Date().toISOString().replace(/[:.]/g, '-')}.json`;
      anchor.click();
      URL.revokeObjectURL(url);
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
          <span><strong>{t('settings.resources_card')}</strong><small>{t('settings.resources_subtitle')}</small></span>
          <StatusPill tone="accent">{t('settings.runtime_revision', { revision: snapshotRevision })}</StatusPill>
        </div>
        <Button variant="secondary" onClick={query.reload} loading={query.refreshing}><IconRefreshCw size={14} />{t('common.refresh')}</Button>
      </PageActions>
      <SuccessNotice message={notice} onDismiss={() => setNotice('')} />
      {query.error && <ErrorState error={query.error} onRetry={query.reload} />}
      {mutationError && !createOpen && !revokeTarget && <ErrorState error={mutationError} />}

      <div className={styles.settingsGrid}>
        <Card title={t('settings.card.key_session')} subtitle={t('settings.card.key_session_subtitle')}>
          <div className={styles.settingsStatus}>
            <IconShield size={20} />
            <span><strong>{adminKeyConfigured ? t('settings.card.key_configured') : t('settings.card.key_not_configured')}</strong><small>{adminKeyConfigured ? t('settings.card.key_loaded_hint') : t('settings.card.key_hint')}</small></span>
          </div>
          <div className={styles.cardActions}>
            <Button variant="secondary" onClick={() => { onClearAdminKey(); setNotice(t('settings.key_cleared')); }} disabled={!adminKeyConfigured}>{t('settings.clear_key')}</Button>
          </div>
        </Card>

        <Card title={t('settings.card.runtime')} subtitle={t('settings.card.runtime_subtitle')}>
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

        <Card title={t('settings.card.export')} subtitle={t('settings.card.export_subtitle')}>
          <div className={styles.settingsStatus}>
            <IconDownload size={20} />
            <span><strong>{t('settings.card.export_redacted')}</strong><small>{t('settings.card.export_redacted_hint')}</small></span>
          </div>
          <div className={styles.cardActions}>
            <Button variant="secondary" onClick={exportConfiguration} loading={mutationBusy}><IconDownload size={14} />{t('settings.export_json')}</Button>
          </div>
        </Card>
      </div>

      <Card variant="flush" title={t('settings.card.keys')} subtitle={t('settings.card.keys_subtitle')} extra={<Button size="sm" variant="primary" onClick={() => setCreateOpen(true)}><IconPlus size={14} />{t('settings.new_key')}</Button>}>
        {data.keys.length === 0 ? <EmptyTable title={t('settings.keys_empty')} /> : (
          <TableScroll label={t('settings.keys_table_aria')}>
            <table className={styles.table}>
              <thead><tr><th>{t('settings.keys_column.name')}</th><th>{t('settings.keys_column.prefix')}</th><th>{t('settings.keys_column.allowed_models')}</th><th>{t('settings.keys_column.created')}</th><th>{t('settings.keys_column.last_used')}</th><th>{t('settings.keys_column.status')}</th><th>{t('common.actions')}</th></tr></thead>
              <tbody>{data.keys.map((key) => (
                <tr key={key.id}>
                  <td><span className={styles.primaryText}><strong>{key.name}</strong><small>{t('settings.row_id', { id: key.id })}</small></span></td>
                  <td><code>{key.key_prefix}…</code></td>
                  <td>{key.allowed_models.length === 0 ? <StatusPill>{t('settings.all_models')}</StatusPill> : <span className={styles.inlineActions}>{key.allowed_models.map((model) => <StatusPill key={model}>{model}</StatusPill>)}</span>}</td>
                  <td>{formatDateTime(key.created_at)}</td>
                  <td>{formatDateTime(key.last_used_at)}</td>
                  <td><StatusPill tone={key.enabled && !key.revoked_at ? 'success' : 'muted'}>{key.revoked_at ? t('settings.key_status.revoked') : key.enabled ? t('settings.key_status.active') : t('settings.key_status.disabled')}</StatusPill></td>
                  <td><span className={styles.inlineActions}>
                    <IconButton label={key.key_recoverable ? t('settings.view_key_aria', { name: key.name }) : t('settings.view_key_unavailable_aria', { name: key.name })} disabled={!key.key_recoverable} onClick={() => revealKey(key)}><IconEye size={16} /></IconButton>
                    <IconButton label={t('settings.revoke_key_aria', { name: key.name })} className={styles.dangerIcon} disabled={!key.enabled || Boolean(key.revoked_at)} onClick={() => setRevokeTarget(key)}><IconTrash2 size={16} /></IconButton>
                  </span></td>
                </tr>
              ))}</tbody>
            </table>
          </TableScroll>
        )}
      </Card>

      <Modal
        open={createOpen}
        title={t('settings.modal.new_key')}
        width={560}
        onClose={() => !mutationBusy && setCreateOpen(false)}
        closeDisabled={mutationBusy}
        footer={<><Button variant="secondary" onClick={() => setCreateOpen(false)} disabled={mutationBusy}>{t('common.cancel')}</Button><Button type="submit" form="virtual-key-editor-form" loading={mutationBusy}><IconKey size={14} />{t('settings.modal.create_key')}</Button></>}
      >
        <VirtualKeyForm busy={mutationBusy} error={mutationError ? localizedApiError(mutationError) : undefined} onSubmit={createKey} />
      </Modal>

      <Modal
        open={Boolean(revealedKey)}
        title={revealedKey ? t('settings.modal.reveal_title', { name: revealedKey.name }) : ''}
        width={520}
        onClose={() => { setRevealedKey(undefined); setCopyStatus('pending'); }}
        footer={<Button variant="secondary" onClick={() => { setRevealedKey(undefined); setCopyStatus('pending'); }}>{t('common.done')}</Button>}
      >
        <div className={styles.secretResult} role="status">
          <IconKey size={22} />
          <div>
            <strong>{copyStatus === 'copied' ? t('settings.api_key_copied') : copyStatus === 'failed' ? t('settings.api_key_copy_denied') : t('settings.modal.reveal_subtitle')}</strong>
            <span>{t('settings.modal.reveal_decrypt_note')}</span>
            {revealedKey && <code className={styles.secretValue}>{revealedKey.key}</code>}
          </div>
          <Button variant="primary" onClick={() => void copyRevealedKey()}><IconCopy size={14} />{t('common.copy_api_key')}</Button>
        </div>
      </Modal>

      <ConfirmDialog
        open={Boolean(revokeTarget)}
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
