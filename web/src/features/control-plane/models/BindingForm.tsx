import type {
  Account,
  AdminErrorShape,
  CatalogStatus,
  GatewayAdminResources,
  GatewayProtocol,
  LogicalModel,
  ModelBinding,
  ModelBindingWriteInput,
  Source,
  SourceModel
} from '@/admin-api';
import { GATEWAY_PROTOCOLS, normalizeAdminError } from '@/admin-api';
import { SelectField, TextField } from '@/components/ui/FormField';
import { StatusPill } from '@/components/ui/StatusPill';
import styles from '@/features/control-plane/ControlPlane.module.scss';
import { statusOptions } from '@/features/control-plane/models/catalog';
import { CheckboxField, ErrorState, FormError, FormGrid } from '@/features/control-plane/shared';
import { PROTOCOL_LABELS } from '@/lib/protocols';
import { useCallback, useEffect, useState, type FormEvent } from 'react';
import { useTranslation } from 'react-i18next';

export function BindingForm({
  record,
  logicalModels,
  sources,
  accounts,
  api,
  busy,
  error,
  onSubmit,
}: {
  record?: ModelBinding;
  logicalModels: LogicalModel[];
  sources: Source[];
  accounts: Account[];
  api: GatewayAdminResources;
  busy: boolean;
  error?: string;
  onSubmit: (input: ModelBindingWriteInput) => void;
}) {
  const { t } = useTranslation('console');
  const [logicalModelId, setLogicalModelId] = useState(record?.logical_model_id ?? logicalModels[0]?.id ?? '');
  const [sourceId, setSourceId] = useState(record?.source_id ?? sources[0]?.id ?? '');
  const availableAccounts = accounts.filter((account) => account.source_id === sourceId);
  const [accountId, setAccountId] = useState(record?.account_id ?? availableAccounts[0]?.id ?? '');
  const [upstreamModelId, setUpstreamModelId] = useState(record?.upstream_model_id ?? '');
  const [protocol, setProtocol] = useState<GatewayProtocol>(record?.protocol ?? 'openai_chat_completions');
  const [status, setStatus] = useState<CatalogStatus>(record?.status ?? 'pending');
  const [priority, setPriority] = useState(record?.priority ?? 100);
  const [enabled, setEnabled] = useState(record?.enabled ?? true);
  const [sourceModels, setSourceModels] = useState<SourceModel[]>([]);
  const [modelsLoading, setModelsLoading] = useState(true);
  const [modelsError, setModelsError] = useState<AdminErrorShape>();
  const [validationError, setValidationError] = useState('');

  const loadModels = useCallback(async (signal: AbortSignal) => {
    if (!sourceId) {
      setSourceModels([]);
      setModelsLoading(false);
      return;
    }
    setModelsLoading(true);
    setModelsError(undefined);
    try {
      const models = await api.sourceModels(sourceId, {}, signal);
      if (!signal.aborted) {
        setSourceModels(models);
        setUpstreamModelId((current) => current || models[0]?.upstream_model_id || '');
      }
    } catch (loadError) {
      if (!signal.aborted) setModelsError(normalizeAdminError(loadError));
    } finally {
      if (!signal.aborted) setModelsLoading(false);
    }
  }, [api, sourceId]);

  useEffect(() => {
    const controller = new AbortController();
    void loadModels(controller.signal);
    return () => controller.abort();
  }, [loadModels]);

  const changeSource = (nextSourceId: string) => {
    setSourceId(nextSourceId);
    setAccountId(accounts.find((account) => account.source_id === nextSourceId)?.id ?? '');
    setUpstreamModelId('');
  };

  const submit = (event: FormEvent) => {
    event.preventDefault();
    if (!logicalModelId || !sourceId || !accountId || !upstreamModelId) {
      setValidationError(t('models.binding_form.validate_required'));
      return;
    }
    if (!Number.isInteger(priority)) {
      setValidationError(t('models.binding_form.validate_priority'));
      return;
    }
    setValidationError('');
    onSubmit({
      logical_model_id: logicalModelId,
      source_id: sourceId,
      account_id: accountId,
      upstream_model_id: upstreamModelId,
      protocol,
      status,
      enabled,
      priority,
    });
  };

  return (
    <form id="binding-editor-form" className={styles.page} onSubmit={submit}>
      <FormGrid>
        <SelectField
          label={t('models.field.lm')}
          value={logicalModelId}
          disabled={busy}
          data={logicalModels.map((model) => ({ value: model.id, label: `${model.display_name} · ${model.id}` }))}
          onChange={setLogicalModelId}
        />
        <SelectField
          label={t('models.field.protocol_in')}
          value={protocol}
          disabled={busy}
          data={GATEWAY_PROTOCOLS.map((item) => ({ value: item, label: PROTOCOL_LABELS[item] }))}
          onChange={(value) => setProtocol(value as GatewayProtocol)}
        />
        <SelectField
          label={t('models.field.source')}
          value={sourceId}
          disabled={busy}
          data={sources.map((source) => ({ value: source.id, label: `${source.display_name} · ${source.id}` }))}
          onChange={changeSource}
        />
        <SelectField
          label={t('models.field.account')}
          value={accountId}
          disabled={busy || availableAccounts.length === 0}
          data={availableAccounts.length === 0
            ? [{ value: '', label: t('models.binding_form.no_account') }]
            : availableAccounts.map((account) => ({ value: account.id, label: `${account.display_name} · ${account.id}` }))}
          onChange={setAccountId}
        />
        <SelectField
          label={t('models.field.source_model')}
          value={upstreamModelId}
          disabled={busy || modelsLoading || sourceModels.length === 0}
          data={modelsLoading
            ? [{ value: '', label: t('models.binding_form.source_model_loading') }]
            : sourceModels.length === 0
              ? [{ value: '', label: t('models.binding_form.no_source_model') }]
              : sourceModels.map((model) => ({
                  value: model.upstream_model_id,
                  label: `${model.upstream_model_id} · ${t(`values.status.${model.confirmation_status}`, { defaultValue: model.confirmation_status })}/${t(`values.availability.${model.availability_status}`, { defaultValue: model.availability_status })}`,
                }))}
          onChange={setUpstreamModelId}
        />
        <SelectField
          label={t('models.field.binding_status')}
          value={status}
          disabled={busy}
          data={statusOptions(record).map((option) => ({ value: option, label: t(`values.status.${option}`) }))}
          onChange={(value) => setStatus(value as CatalogStatus)}
        />
        <TextField label={t('models.field.priority')} hint={t('models.binding_form.priority_hint')} type="number" step={1} value={priority} disabled={busy} onChange={(event) => setPriority(Number(event.target.value))} />
        <div className={styles.field}><label>{t('models.field.binding_id')}</label><StatusPill>{record?.id ?? t('models.binding_form.binding_id_auto')}</StatusPill></div>
        <div className={styles.fullWidth}><CheckboxField checked={enabled} disabled={busy} onChange={setEnabled} label={t('models.field.enable_binding')} /></div>
      </FormGrid>
      {modelsError && <ErrorState error={modelsError} onRetry={() => void loadModels(new AbortController().signal)} />}
      <FormError message={validationError || error} />
    </form>
  );
}
