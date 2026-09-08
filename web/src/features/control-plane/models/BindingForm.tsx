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
        <SelectField label={t('models.field.lm')} value={logicalModelId} disabled={busy} onChange={(event) => setLogicalModelId(event.target.value)}>
          {logicalModels.map((model) => <option key={model.id} value={model.id}>{model.display_name} · {model.id}</option>)}
        </SelectField>
        <SelectField label={t('models.field.protocol_in')} value={protocol} disabled={busy} onChange={(event) => setProtocol(event.target.value as GatewayProtocol)}>
          {GATEWAY_PROTOCOLS.map((item) => <option key={item} value={item}>{PROTOCOL_LABELS[item]}</option>)}
        </SelectField>
        <SelectField label={t('models.field.source')} value={sourceId} disabled={busy} onChange={(event) => changeSource(event.target.value)}>
          {sources.map((source) => <option key={source.id} value={source.id}>{source.display_name} · {source.id}</option>)}
        </SelectField>
        <SelectField label={t('models.field.account')} value={accountId} disabled={busy || availableAccounts.length === 0} onChange={(event) => setAccountId(event.target.value)}>
          {availableAccounts.length === 0 && <option value="">{t('models.binding_form.no_account')}</option>}
          {availableAccounts.map((account) => <option key={account.id} value={account.id}>{account.display_name} · {account.id}</option>)}
        </SelectField>
        <SelectField label={t('models.field.source_model')} value={upstreamModelId} disabled={busy || modelsLoading || sourceModels.length === 0} onChange={(event) => setUpstreamModelId(event.target.value)}>
          {modelsLoading && <option value="">{t('models.binding_form.source_model_loading')}</option>}
          {!modelsLoading && sourceModels.length === 0 && <option value="">{t('models.binding_form.no_source_model')}</option>}
          {sourceModels.map((model) => <option key={model.upstream_model_id} value={model.upstream_model_id}>{model.upstream_model_id} · {t(`values.status.${model.confirmation_status}`, { defaultValue: model.confirmation_status })}/{t(`values.availability.${model.availability_status}`, { defaultValue: model.availability_status })}</option>)}
        </SelectField>
        <SelectField label={t('models.field.binding_status')} value={status} disabled={busy} onChange={(event) => setStatus(event.target.value as CatalogStatus)}>
          {statusOptions(record).map((option) => <option key={option} value={option}>{t(`values.status.${option}`)}</option>)}
        </SelectField>
        <TextField label={t('models.field.priority')} hint={t('models.binding_form.priority_hint')} type="number" step={1} value={priority} disabled={busy} onChange={(event) => setPriority(Number(event.target.value))} />
        <div className={styles.field}><label>{t('models.field.binding_id')}</label><StatusPill>{record?.id ?? t('models.binding_form.binding_id_auto')}</StatusPill></div>
        <div className={styles.fullWidth}><CheckboxField checked={enabled} disabled={busy} onChange={setEnabled} label={t('models.field.enable_binding')} /></div>
      </FormGrid>
      {modelsError && <ErrorState error={modelsError} onRetry={() => void loadModels(new AbortController().signal)} />}
      <FormError message={validationError || error} />
    </form>
  );
}
