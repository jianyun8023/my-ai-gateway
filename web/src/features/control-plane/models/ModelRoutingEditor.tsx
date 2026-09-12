import { Accordion } from '@mantine/core';
import { useCallback, useLayoutEffect, useRef, useState, type DragEvent, type FormEvent } from 'react';
import { useTranslation } from 'react-i18next';
import {
  GATEWAY_PROTOCOLS,
  type GatewayAdminResources,
  type GatewayProtocol,
  type ModelRoutingConfiguration,
  type ModelRoutingLineInput,
  type ModelRoutingWriteInput,
  type SourceModel,
  type SourceModelCapability,
} from '@/admin-api';
import { Button } from '@/components/ui/Button';
import { Card } from '@/components/ui/Card';
import { DetailItem, DetailList } from '@/components/ui/DetailList';
import { SelectField, TextField } from '@/components/ui/FormField';
import { IconMenu, IconPlus, IconTrash2 } from '@/components/ui/icons';
import { LoadingState } from '@/components/ui/LoadingState';
import { Notice } from '@/components/ui/Notice';
import { StatusPill, type StatusTone } from '@/components/ui/StatusPill';
import type { CatalogData } from '@/features/control-plane/models/catalog';
import { DrawerSection, ErrorState, FormError, FormGrid, ProtocolPill, Toggle } from '@/features/control-plane/shared';
import { useAdminQuery } from '@/hooks/useAdminQuery';
import styles from './ModelRoutingEditor.module.scss';

interface DraftLine extends ModelRoutingLineInput {
  key: string;
}

type ProtocolMode = 'native' | 'adapter' | 'mixed' | 'unsupported' | 'unknown' | 'unavailable';

const protocolTone: Record<ProtocolMode, StatusTone> = {
  native: 'success', adapter: 'warning', mixed: 'warning', unsupported: 'muted', unknown: 'muted', unavailable: 'danger',
};

const modelKey = (sourceId: string, upstreamModelId: string) => JSON.stringify([sourceId, upstreamModelId]);
const blankLine = (key: string): DraftLine => ({ key, source_id: '', account_id: '', upstream_model_id: '' });
const optionalInteger = (value: string, minimum: number) => value.trim() === ''
  || (Number.isSafeInteger(Number(value)) && Number(value) >= minimum);
const resourceLabel = (record: { id: string; display_name: string }, records: Array<{ id: string; display_name: string }>) => {
  const name = record.display_name.trim();
  if (!name) return record.id;
  return records.some((item) => item.id !== record.id && item.display_name.trim() === name) ? `${name} · ${record.id}` : name;
};

export function ModelRoutingEditor({
  api, data, configuration, busy, error, onSubmit,
}: {
  api: GatewayAdminResources;
  data: CatalogData;
  configuration?: ModelRoutingConfiguration;
  busy: boolean;
  error?: string;
  onSubmit: (id: string | undefined, input: ModelRoutingWriteInput) => void;
}) {
  const { t } = useTranslation('console');
  const record = configuration?.logical_model;
  const [publicName, setPublicName] = useState(record?.public_name ?? '');
  const [enabled, setEnabled] = useState(record?.enabled ?? true);
  const [lines, setLines] = useState<DraftLine[]>(() => configuration
    ? configuration.lines.map((line, index) => ({ ...line, key: `line-${index}` }))
    : [blankLine('line-0')]);
  const nextLineKey = useRef(configuration?.lines.length ?? 1);
  const [timeout, setTimeout] = useState(configuration?.request_timeout_ms?.toString() ?? '');
  const [maxRetries, setMaxRetries] = useState(configuration?.max_retries?.toString() ?? '');
  const [showValidation, setShowValidation] = useState(false);
  const [validationError, setValidationError] = useState('');
  const [draggedKey, setDraggedKey] = useState<string | null>(null);
  const [announcement, setAnnouncement] = useState('');
  const [healthCheckedAt] = useState(Date.now);
  const lineElements = useRef(new Map<string, HTMLLIElement>());
  const focusAfterMove = useRef<string | null>(null);
  useLayoutEffect(() => {
    if (focusAfterMove.current !== null) {
      lineElements.current.get(focusAfterMove.current)?.querySelector<HTMLButtonElement>('button[draggable="true"]')?.focus();
      focusAfterMove.current = null;
    }
  }, [lines]);

  // Each query key describes the current draft only. Changing a Source/model
  // cancels the old request and hides its result before a late response arrives.
  const sourceIdsKey = JSON.stringify([...new Set(lines.map((line) => line.source_id).filter(Boolean))].sort());
  const loadModels = useCallback(async (signal: AbortSignal) => {
    const sourceIds: string[] = JSON.parse(sourceIdsKey);
    return Object.fromEntries(await Promise.all(sourceIds.map(async (sourceId) => [
      sourceId,
      await api.sourceModels(sourceId, { confirmationStatus: 'confirmed' }, signal),
    ] as const))) as Record<string, SourceModel[]>;
  }, [api, sourceIdsKey]);
  const modelsQuery = useAdminQuery({ load: loadModels, queryKey: sourceIdsKey });

  const selectedModelsKey = JSON.stringify([...new Set(lines
    .filter((line) => line.source_id && line.upstream_model_id)
    .map((line) => modelKey(line.source_id, line.upstream_model_id)))].sort());
  const loadCapabilities = useCallback(async (signal: AbortSignal) => {
    const selectedModels: string[] = JSON.parse(selectedModelsKey);
    return Object.fromEntries(await Promise.all(selectedModels.map(async (key) => {
      const [sourceId, upstreamModelId]: [string, string] = JSON.parse(key);
      return [key, await api.sourceModelCapabilities(sourceId, upstreamModelId, signal)] as const;
    }))) as Record<string, SourceModelCapability[]>;
  }, [api, selectedModelsKey]);
  const capabilitiesQuery = useAdminQuery({ load: loadCapabilities, queryKey: selectedModelsKey });

  const modelForLine = (line: DraftLine) => modelsQuery.data?.[line.source_id]
    ?.find((model) => model.upstream_model_id === line.upstream_model_id && model.confirmation_status === 'confirmed');
  const protocolForLine = (line: DraftLine, protocol: GatewayProtocol): ProtocolMode => {
    if (!line.source_id || !line.account_id || !line.upstream_model_id || modelsQuery.loading || capabilitiesQuery.loading
      || modelsQuery.error || capabilitiesQuery.error) return 'unknown';
    const model = modelForLine(line);
    const source = data.sources.find((item) => item.id === line.source_id);
    const account = data.accounts.find((item) => item.id === line.account_id && item.source_id === line.source_id);
    if (!model || model.availability_status === 'unavailable' || !source?.enabled || !account?.enabled) return 'unavailable';
    if (model.availability_status === 'unknown') return 'unknown';
    const capability = capabilitiesQuery.data?.[modelKey(line.source_id, line.upstream_model_id)]
      ?.find((item) => item.protocol === protocol);
    if (!capability || capability.status === 'pending') return 'unknown';
    if (capability.status !== 'confirmed') return 'unavailable';
    if (capability.mode === 'native') return source.endpoints[protocol] ? 'native' : 'unavailable';
    if (capability.mode !== 'adapter') return capability.mode;
    // A declared adapter is not proof that an adapter is registered. Only a
    // matching published runtime path can substantiate this display mode.
    const runtimeAdapter = data.capabilities.data.some((row) => row.source.source_id === line.source_id
      && row.account.account_id === line.account_id && row.upstream_model_id === line.upstream_model_id
      && row.protocols.some((cell) => cell.protocol_in === protocol && cell.status === 'routable'
        && cell.mode === 'adapter' && cell.adapter === capability.adapter));
    return runtimeAdapter ? 'adapter' : 'unavailable';
  };
  const protocolSummary = (protocol: GatewayProtocol): ProtocolMode => {
    const modes = lines.map((line) => protocolForLine(line, protocol));
    if (modes.includes('native') && modes.includes('adapter')) return 'mixed';
    if (modes.includes('native')) return 'native';
    if (modes.includes('adapter')) return 'adapter';
    if (modes.length === 0 || modes.includes('unknown')) return 'unknown';
    return modes.includes('unavailable') ? 'unavailable' : 'unsupported';
  };

  const updateLine = (key: string, change: Partial<ModelRoutingLineInput>) => {
    if (busy) return;
    setValidationError('');
    setLines((current) => current.map((line) => line.key === key ? { ...line, ...change } : line));
  };
  const moveLine = (key: string, targetIndex: number, retainFocus = false) => {
    if (busy || targetIndex < 0 || targetIndex >= lines.length) return;
    const fromIndex = lines.findIndex((line) => line.key === key);
    if (fromIndex < 0 || fromIndex === targetIndex) return;
    if (retainFocus) focusAfterMove.current = key;
    setLines((current) => {
      const reordered = [...current];
      const [moved] = reordered.splice(fromIndex, 1);
      reordered.splice(targetIndex, 0, moved);
      return reordered;
    });
    setAnnouncement(t('models.v3.editor.moved', { from: fromIndex + 1, to: targetIndex + 1 }));
    setValidationError('');
  };
  const dragStart = (event: DragEvent<HTMLButtonElement>, key: string) => {
    if (busy) { event.preventDefault(); return; }
    event.dataTransfer.effectAllowed = 'move';
    event.dataTransfer.setData('text/plain', key);
    setDraggedKey(key);
  };
  const submit = (event: FormEvent) => {
    event.preventDefault();
    if (busy) return;
    setShowValidation(true);
    setValidationError('');
    if (!publicName.trim() || !optionalInteger(timeout, 1) || !optionalInteger(maxRetries, 0)
      || lines.some((line) => !line.source_id || !line.account_id || !line.upstream_model_id)) return;
    if (enabled && lines.length === 0) {
      setValidationError(t('models.v3.editor.validate_lines'));
      return;
    }
    const identities = lines.map((line) => JSON.stringify([line.source_id, line.account_id, line.upstream_model_id]));
    if (new Set(identities).size !== identities.length) {
      setValidationError(t('models.v3.editor.validate_duplicate'));
      return;
    }
    if (modelsQuery.loading) {
      setValidationError(t('models.v3.editor.wait_for_models'));
      return;
    }
    if (modelsQuery.data && lines.some((line) => {
      const model = modelForLine(line);
      return !model || model.availability_status !== 'available';
    })) {
      setValidationError(t('models.v3.editor.validate_confirmed_models'));
      return;
    }
    onSubmit(record?.id, {
      public_name: publicName.trim(),
      display_name: record?.display_name ?? publicName.trim(),
      enabled,
      lines: lines.map(({ source_id, account_id, upstream_model_id }) => ({ source_id, account_id, upstream_model_id })),
      request_timeout_ms: timeout.trim() === '' ? null : Number(timeout),
      max_retries: maxRetries.trim() === '' ? null : Number(maxRetries),
    });
  };

  const savedBindings = data.bindings.filter((binding) => binding.logical_model_id === record?.id);
  const savedRoutes = data.routes.filter((route) => route.logical_model_id === record?.id);
  const savedRouteIds = new Set(savedRoutes.map((route) => route.id));
  const runtimeRows = data.capabilities.data.filter((row) => savedRouteIds.has(row.route_id));
  const requiredError = (value: string) => showValidation && !value.trim() ? t('models.v3.editor.required') : undefined;

  return (
    <form id="model-routing-editor-form" className={styles.form} onSubmit={submit} noValidate aria-busy={busy}>
      <DrawerSection title={t('models.v3.editor.basic_info')}>
        <div className={styles.basicInfo}>
          <TextField label={t('models.v3.editor.public_name')} value={publicName} disabled={busy}
            error={requiredError(publicName)} autoComplete="off"
            onChange={(event) => setPublicName(event.target.value)} />
          <div className={styles.enabled}>
            <span>{t('common.status')}</span>
            <Toggle checked={enabled} disabled={busy} onChange={setEnabled} label={t('models.v3.editor.enable_model')} />
          </div>
        </div>
      </DrawerSection>

      <DrawerSection title={t('models.v3.lines')}>
        <div className={styles.sectionContent}>
          <p className={styles.hint}>{t('models.v3.editor.order_hint')}</p>
          {configuration?.strategy === 'primary_then_weighted_fallback'
            && <Notice tone="warning">{t('models.v3.editor.weighted_change')}</Notice>}
          <ol className={styles.lines} aria-label={t('models.v3.lines')}>
            {lines.map((line, index) => {
              const lineTitle = index === 0 ? t('models.v3.primary') : t('models.v3.backup', { index });
              const accounts = data.accounts.filter((account) => account.source_id === line.source_id);
              const sourceModels = modelsQuery.data?.[line.source_id]?.filter((model) => model.confirmation_status === 'confirmed') ?? [];
              const selectedModelExists = sourceModels.some((model) => model.upstream_model_id === line.upstream_model_id);
              const selectedAccount = accounts.find((account) => account.id === line.account_id);
              const selectedSource = data.sources.find((source) => source.id === line.source_id);
              const selectedModel = modelForLine(line);
              const coolingDown = selectedAccount?.cooldown_until && Date.parse(selectedAccount.cooldown_until) > healthCheckedAt;
              const health = !line.source_id || !line.account_id || !line.upstream_model_id || modelsQuery.loading || modelsQuery.error
                ? 'unknown'
                : !selectedSource?.enabled || !selectedAccount?.enabled ? 'disabled'
                  : !selectedModel || selectedModel.availability_status === 'unavailable' || coolingDown ? 'unavailable'
                    : selectedModel.availability_status === 'unknown' || selectedAccount.health_status === 'unknown' || !selectedAccount.health_status ? 'unknown'
                      : selectedAccount.health_status === 'healthy' ? 'healthy' : 'unavailable';
              return (
                <li key={line.key} data-line-key={line.key} data-dragging={draggedKey === line.key || undefined}
                  ref={(element) => {
                    if (element) lineElements.current.set(line.key, element);
                    else lineElements.current.delete(line.key);
                  }}
                  onDragOver={(event) => {
                    if (!busy && draggedKey !== null) { event.preventDefault(); event.dataTransfer.dropEffect = 'move'; }
                  }}
                  onDrop={(event) => {
                    if (busy || draggedKey === null) return;
                    event.preventDefault();
                    moveLine(draggedKey, index);
                    setDraggedKey(null);
                  }}>
                  <Card className={styles.lineCard} title={(
                    <span className={styles.lineTitle}>
                      <StatusPill tone={index === 0 ? 'accent' : 'muted'}>{index + 1}</StatusPill>
                      <span>{lineTitle}</span>
                    </span>
                  )} titleMeta={<StatusPill tone={health === 'healthy' ? 'success' : health === 'unavailable' ? 'warning' : 'muted'}>{t(`models.v3.${health}`)}</StatusPill>}
                  extra={(
                    <div className={styles.lineActions}>
                      <Button size="sm" variant="ghost" draggable={!busy} disabled={busy}
                        aria-label={t('models.v3.editor.drag_aria', { index: index + 1 })}
                        onDragStart={(event) => dragStart(event, line.key)} onDragEnd={() => setDraggedKey(null)}>
                        <IconMenu size={14} />{t('models.v3.editor.drag')}
                      </Button>
                      <Button size="sm" variant="secondary" disabled={busy || index === 0}
                        aria-label={t('models.v3.editor.move_up_aria', { index: index + 1 })}
                        onClick={() => moveLine(line.key, index - 1, true)}>{t('models.v3.editor.move_up')}</Button>
                      <Button size="sm" variant="secondary" disabled={busy || index === lines.length - 1}
                        aria-label={t('models.v3.editor.move_down_aria', { index: index + 1 })}
                        onClick={() => moveLine(line.key, index + 1, true)}>{t('models.v3.editor.move_down')}</Button>
                      <Button size="sm" variant="ghost" disabled={busy}
                        aria-label={t('models.v3.editor.remove_aria', { index: index + 1 })}
                        onClick={() => {
                          if (busy) return;
                          setLines((current) => current.filter((item) => item.key !== line.key));
                          setValidationError('');
                          setAnnouncement(t('models.v3.editor.removed', { index: index + 1 }));
                        }}><IconTrash2 size={14} />{t('models.v3.editor.remove')}</Button>
                    </div>
                  )}>
                    <FormGrid>
                      <SelectField label={t('models.field.source')} aria-label={`${lineTitle} · ${t('models.field.source')}`}
                        value={line.source_id || null} disabled={busy || data.sources.length === 0}
                        placeholder={t(data.sources.length ? 'models.v3.editor.select_source' : 'models.v3.editor.no_sources')}
                        error={requiredError(line.source_id)}
                        data={data.sources.map((source) => ({ value: source.id, label: resourceLabel(source, data.sources), disabled: !source.enabled }))}
                        onChange={(sourceId) => updateLine(line.key, {
                          source_id: sourceId,
                          account_id: data.accounts.find((account) => account.source_id === sourceId && account.enabled)?.id ?? '',
                          upstream_model_id: '',
                        })} />
                      <SelectField label={t('models.field.account')} aria-label={`${lineTitle} · ${t('models.field.account')}`}
                        value={line.account_id || null} disabled={busy || !line.source_id || accounts.length === 0}
                        placeholder={t(accounts.length ? 'models.v3.editor.select_account' : 'models.v3.editor.no_accounts')}
                        error={requiredError(line.account_id)}
                        data={accounts.map((account) => ({ value: account.id, label: resourceLabel(account, accounts), disabled: !account.enabled }))}
                        onChange={(accountId) => updateLine(line.key, { account_id: accountId })} />
                      <SelectField className={styles.fullWidth} label={t('models.field.upstream_model')}
                        aria-label={`${lineTitle} · ${t('models.field.upstream_model')}`}
                        value={line.upstream_model_id || null} disabled={busy || !line.source_id || modelsQuery.loading || sourceModels.length === 0}
                        placeholder={t(modelsQuery.loading && line.source_id ? 'models.v3.editor.loading_models' : sourceModels.length ? 'models.v3.editor.select_model' : 'models.v3.editor.no_confirmed_models')}
                        error={requiredError(line.upstream_model_id)}
                        data={[
                          ...(line.upstream_model_id && !selectedModelExists ? [{
                            value: line.upstream_model_id,
                            label: modelsQuery.loading ? line.upstream_model_id : `${line.upstream_model_id} · ${t('models.v3.unavailable')}`,
                            disabled: true,
                          }] : []),
                          ...sourceModels.map((model) => ({
                            value: model.upstream_model_id,
                            label: model.availability_status === 'available' ? model.upstream_model_id
                              : `${model.upstream_model_id} · ${t(`values.availability.${model.availability_status}`)}`,
                            disabled: model.availability_status !== 'available',
                          })),
                        ]}
                        onChange={(upstreamModelId) => updateLine(line.key, { upstream_model_id: upstreamModelId })} />
                    </FormGrid>
                  </Card>
                </li>
              );
            })}
          </ol>
          {lines.length === 0 && <p className={styles.hint}>{t('models.v3.no_lines')}</p>}
          <Button variant="secondary" fullWidth disabled={busy || data.sources.length === 0} onClick={() => {
            if (busy) return;
            const key = `line-${nextLineKey.current++}`;
            setLines((current) => [...current, blankLine(key)]);
            setValidationError('');
          }}><IconPlus size={16} />{t(lines.length ? 'models.v3.editor.add_backup' : 'models.v3.editor.add_line')}</Button>
          {modelsQuery.loading && sourceIdsKey !== '[]' && <LoadingState layout="inline" label={t('models.v3.editor.loading_models')} />}
          {modelsQuery.error && <ErrorState error={modelsQuery.error} onRetry={busy ? undefined : modelsQuery.reload} />}
          <span className={styles.srOnly} role="status" aria-live="polite">{announcement}</span>
        </div>
      </DrawerSection>

      <DrawerSection title={t('models.v3.editor.request_settings')}>
        <FormGrid>
          <TextField label={t('models.v3.editor.timeout')} type="number" min={1} step={1} value={timeout} disabled={busy}
            placeholder={t('models.v3.editor.inherit_timeout')} hint={t('models.v3.editor.timeout_hint')}
            error={showValidation && !optionalInteger(timeout, 1) ? t('models.v3.editor.validate_timeout') : undefined}
            onChange={(event) => setTimeout(event.target.value)} />
          <TextField label={t('models.v3.editor.max_retries')} type="number" min={0} step={1} value={maxRetries} disabled={busy}
            placeholder={t('models.v3.editor.all_backups')} hint={t('models.v3.editor.retries_hint')}
            error={showValidation && !optionalInteger(maxRetries, 0) ? t('models.v3.editor.validate_retries') : undefined}
            onChange={(event) => setMaxRetries(event.target.value)} />
        </FormGrid>
      </DrawerSection>

      <DrawerSection title={t('models.v3.editor.protocols')}>
        <div className={styles.sectionContent}>
          <div className={styles.protocols}>
            {GATEWAY_PROTOCOLS.map((protocol) => {
              const mode = protocolSummary(protocol);
              return <div key={protocol} className={styles.protocol} data-protocol={protocol}>
                <ProtocolPill protocol={protocol} /><StatusPill tone={protocolTone[mode]}>{t(`models.v3.${mode}`)}</StatusPill>
              </div>;
            })}
          </div>
          {capabilitiesQuery.loading && selectedModelsKey !== '[]' && <LoadingState layout="inline" label={t('models.v3.editor.loading_capabilities')} />}
          {capabilitiesQuery.error && <ErrorState error={capabilitiesQuery.error} onRetry={busy ? undefined : capabilitiesQuery.reload} />}
          <p className={styles.hint}>{t('models.v3.editor.protocols_hint')}</p>
        </div>
      </DrawerSection>

      <Accordion order={3} keepMounted={false}>
        <Accordion.Item value="advanced">
          <Accordion.Control>{t('models.v3.editor.advanced')}</Accordion.Control>
          <Accordion.Panel>
            <div className={styles.sectionContent}>
              <p className={styles.hint}>{t('models.v3.editor.advanced_hint')}</p>
              <DetailList>
                <DetailItem label={t('models.field.lm_id')}><code>{record?.id ?? t('models.v3.editor.assigned_on_save')}</code></DetailItem>
                <DetailItem label={t('models.field.strategy')}>{configuration?.strategy ?? t('models.v3.editor.ordered_strategy')}</DetailItem>
                <DetailItem label={t('models.v3.editor.bindings')}><span className={styles.inline}>{savedBindings.length ? savedBindings.map((binding) => <code key={binding.id}>{binding.id}</code>) : '—'}</span></DetailItem>
                <DetailItem label={t('models.v3.editor.route_rules')}><span className={styles.inline}>{savedRoutes.length ? savedRoutes.map((route) => <code key={route.id}>{route.id}</code>) : '—'}</span></DetailItem>
              </DetailList>
              {savedBindings.map((binding) => <Card key={binding.id} title={`Binding ${binding.id}`}>
                <DetailList>
                  <DetailItem label={t('models.v3.editor.priority')}>{binding.priority}</DetailItem>
                  <DetailItem label={t('models.field.source_account')}><code>{binding.source_id} / {binding.account_id}</code></DetailItem>
                  <DetailItem label={t('models.v3.editor.mapping')}><code>{record?.public_name} → {binding.upstream_model_id}</code></DetailItem>
                  <DetailItem label={t('models.field.protocol')}><ProtocolPill protocol={binding.protocol} /></DetailItem>
                </DetailList>
              </Card>)}
              <DrawerSection title={t('models.v3.editor.runtime_configuration')}>
                {record ? <pre className={styles.runtime}>{JSON.stringify({
                  configuration,
                  bindings: savedBindings,
                  routes: savedRoutes,
                  runtime: {
                    snapshot_revision: data.capabilities.snapshot_revision,
                    snapshot_generated_at: data.capabilities.snapshot_generated_at,
                    data: runtimeRows,
                  },
                }, null, 2)}</pre> : <p className={styles.hint}>{t('models.v3.editor.not_saved')}</p>}
              </DrawerSection>
            </div>
          </Accordion.Panel>
        </Accordion.Item>
      </Accordion>
      <FormError message={validationError || error} />
    </form>
  );
}
