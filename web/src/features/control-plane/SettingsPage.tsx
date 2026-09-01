import { useCallback, useState, type FormEvent } from 'react';
import type {
  AdminErrorShape,
  CapabilityMatrixResponse,
  GatewayAdminResources,
  RuntimeReloadResult,
  VirtualKey,
} from '@/admin-api';
import { normalizeAdminError } from '@/admin-api';
import { Button } from '@/components/ui/Button';
import { Card } from '@/components/ui/Card';
import { Modal } from '@/components/ui/Modal';
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
  IconButton,
  LoadingState,
  PageActions,
  StatusPill,
  SuccessNotice,
  TableScroll,
  TextAreaField,
  TextField,
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
  const [name, setName] = useState('');
  const [allowedModels, setAllowedModels] = useState('');
  const [validationError, setValidationError] = useState('');

  const submit = (event: FormEvent) => {
    event.preventDefault();
    if (!name.trim()) {
      setValidationError('Key 名称不能为空。');
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
        <TextField label="Key 名称" value={name} disabled={busy} onChange={(event) => setName(event.target.value)} autoComplete="off" />
        <TextAreaField label="允许的模型" hint="逗号分隔；留空表示不限制模型。" value={allowedModels} disabled={busy} onChange={(event) => setAllowedModels(event.target.value)} />
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
      setNotice('Virtual Key 已创建并已加密保存，可随时再次查看。');
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
    }, `Virtual Key ${revokeTarget.name} 已撤销。`);
  };

  const reloadRuntime = () => {
    void mutate(async () => {
      const result = await api.reloadRuntime();
      setReloadResult(result);
      query.reload();
    }, 'Runtime snapshot 已从 PostgreSQL 重新加载。');
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
    }, '已导出当前 Admin 资源快照；文件不包含凭据引用。');
  };

  if (query.loading && !data) return <LoadingState label="正在加载 Settings…" />;
  if (query.error && !data) return <ErrorState error={query.error} onRetry={query.reload} />;
  if (!data) return null;

  const snapshotRevision = reloadResult?.snapshot_revision ?? data.capabilities.snapshot_revision;
  const snapshotGeneratedAt = reloadResult?.snapshot_generated_at ?? data.capabilities.snapshot_generated_at;

  return (
    <section className={styles.page} data-od-id="page-settings">
      <PageActions>
        <div className={styles.snapshotMeta}>
          <span><strong>Admin resources</strong><small>真实控制面契约</small></span>
          <StatusPill tone="accent">runtime revision {snapshotRevision}</StatusPill>
        </div>
        <Button variant="secondary" onClick={query.reload} loading={query.refreshing}><IconRefreshCw size={14} />刷新</Button>
      </PageActions>
      <SuccessNotice message={notice} onDismiss={() => setNotice('')} />
      {query.error && <ErrorState error={query.error} onRetry={query.reload} />}
      {mutationError && !createOpen && !revokeTarget && <ErrorState error={mutationError} />}

      <div className={styles.settingsGrid}>
        <Card title="Admin Key 会话" subtitle="仅保存在当前标签页的 sessionStorage，并只用于 /admin/* Authorization">
          <div className={styles.settingsStatus}>
            <IconShield size={20} />
            <span><strong>{adminKeyConfigured ? '当前标签页已配置 Admin Key' : '当前标签页未配置 Admin Key'}</strong><small>本页成功加载仅表示 Admin API 当前可访问，不推断后端是否启用了鉴权。</small></span>
          </div>
          <div className={styles.cardActions}>
            <Button variant="secondary" onClick={() => { onClearAdminKey(); setNotice('当前标签页的 Admin Key 已清除。'); }} disabled={!adminKeyConfigured}>清除会话 Key</Button>
          </div>
        </Card>

        <Card title="Runtime snapshot" subtitle="事实来源：GET /admin/capabilities；刷新操作：POST /admin/config/reload">
          <DetailList>
            <DetailItem label="Revision"><code>{snapshotRevision}</code></DetailItem>
            <DetailItem label="Generated at">{formatDateTime(snapshotGeneratedAt)}</DetailItem>
            <DetailItem label="Fact source"><StatusPill tone="accent">{data.capabilities.fact_source}</StatusPill></DetailItem>
            <DetailItem label="Published rows">{data.capabilities.data.length}</DetailItem>
          </DetailList>
          <div className={styles.cardActions}>
            <Button variant="secondary" onClick={reloadRuntime} loading={mutationBusy}><IconDatabase size={14} />重新加载 runtime</Button>
          </div>
        </Card>

        <Card title="配置导出" subtitle="组合当前 Sources、Accounts、LogicalModels、Bindings、Routes 与 runtime revision">
          <div className={styles.settingsStatus}>
            <IconDownload size={20} />
            <span><strong>脱敏 JSON</strong><small>导出结果移除 Account credential_env，不包含密文、Authorization、API Key 或请求正文。</small></span>
          </div>
          <div className={styles.cardActions}>
            <Button variant="secondary" onClick={exportConfiguration} loading={mutationBusy}><IconDownload size={14} />导出 JSON</Button>
          </div>
        </Card>
      </div>

      <Card variant="flush" title="Virtual Keys" subtitle="认证使用不可逆哈希；原始值加密保存，需 Admin Key 才能查看和复制" extra={<Button size="sm" variant="primary" onClick={() => setCreateOpen(true)}><IconPlus size={14} />新建 Key</Button>}>
        {data.keys.length === 0 ? <EmptyTable title="尚无 Virtual Key" /> : (
          <TableScroll label="Virtual Key 表格">
            <table className={styles.table}>
              <thead><tr><th>名称</th><th>Prefix</th><th>允许模型</th><th>创建时间</th><th>最近使用</th><th>状态</th><th>操作</th></tr></thead>
              <tbody>{data.keys.map((key) => (
                <tr key={key.id}>
                  <td><span className={styles.primaryText}><strong>{key.name}</strong><small>ID {key.id}</small></span></td>
                  <td><code>{key.key_prefix}…</code></td>
                  <td>{key.allowed_models.length === 0 ? <StatusPill>all models</StatusPill> : <span className={styles.inlineActions}>{key.allowed_models.map((model) => <StatusPill key={model}>{model}</StatusPill>)}</span>}</td>
                  <td>{formatDateTime(key.created_at)}</td>
                  <td>{formatDateTime(key.last_used_at)}</td>
                  <td><StatusPill tone={key.enabled && !key.revoked_at ? 'success' : 'muted'}>{key.revoked_at ? 'revoked' : key.enabled ? 'active' : 'disabled'}</StatusPill></td>
                  <td><span className={styles.inlineActions}>
                    <IconButton label={key.key_recoverable ? `查看 ${key.name} API Key` : `${key.name} 不可查看，需轮换`} disabled={!key.key_recoverable} onClick={() => revealKey(key)}><IconEye size={16} /></IconButton>
                    <IconButton label={`撤销 ${key.name}`} className={styles.dangerIcon} disabled={!key.enabled || Boolean(key.revoked_at)} onClick={() => setRevokeTarget(key)}><IconTrash2 size={16} /></IconButton>
                  </span></td>
                </tr>
              ))}</tbody>
            </table>
          </TableScroll>
        )}
      </Card>

      <Modal
        open={createOpen}
        title="新建 Virtual Key"
        width={560}
        onClose={() => !mutationBusy && setCreateOpen(false)}
        closeDisabled={mutationBusy}
        footer={<><Button variant="secondary" onClick={() => setCreateOpen(false)} disabled={mutationBusy}>取消</Button><Button type="submit" form="virtual-key-editor-form" loading={mutationBusy}><IconKey size={14} />创建 Key</Button></>}
      >
        <VirtualKeyForm busy={mutationBusy} error={mutationError?.message} onSubmit={createKey} />
      </Modal>

      <Modal
        open={Boolean(revealedKey)}
        title={revealedKey ? `Virtual Key · ${revealedKey.name}` : 'Virtual Key'}
        width={520}
        onClose={() => { setRevealedKey(undefined); setCopyStatus('pending'); }}
        footer={<Button variant="secondary" onClick={() => { setRevealedKey(undefined); setCopyStatus('pending'); }}>完成</Button>}
      >
        <div className={styles.secretResult} role="status">
          <IconKey size={22} />
          <div>
            <strong>{copyStatus === 'copied' ? 'API Key 已复制到剪贴板' : copyStatus === 'failed' ? '浏览器未允许自动复制' : 'API Key 可查看和复制'}</strong>
            <span>该值只通过 Admin API 解密返回；数据面认证仍使用数据库中的不可逆哈希。</span>
            {revealedKey && <code className={styles.secretValue}>{revealedKey.key}</code>}
          </div>
          <Button variant="primary" onClick={() => void copyRevealedKey()}><IconCopy size={14} />复制 API Key</Button>
        </div>
      </Modal>

      <ConfirmDialog
        open={Boolean(revokeTarget)}
        title="撤销 Virtual Key"
        description={revokeTarget ? <div className={styles.page}>确认撤销 <code className={styles.mono}>{revokeTarget.name}</code>？已撤销 Key 不能恢复。<FormError message={mutationError?.message} /></div> : null}
        confirmLabel="撤销"
        danger
        busy={mutationBusy}
        onCancel={() => !mutationBusy && setRevokeTarget(undefined)}
        onConfirm={revokeKey}
      />
    </section>
  );
}
