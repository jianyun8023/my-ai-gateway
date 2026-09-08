import type {
  GatewayProtocol,
  LogicalModel,
  Route,
  RouteWriteInput
} from '@/admin-api';
import { GATEWAY_PROTOCOLS } from '@/admin-api';
import { SelectField, TextField } from '@/components/ui/FormField';
import { StatusPill } from '@/components/ui/StatusPill';
import styles from '@/features/control-plane/ControlPlane.module.scss';
import { CheckboxField, FormError, FormGrid } from '@/features/control-plane/shared';
import { PROTOCOL_LABELS } from '@/lib/protocols';
import { useState, type FormEvent } from 'react';
import { useTranslation } from 'react-i18next';

export function RouteForm({
  record,
  logicalModels,
  busy,
  error,
  onSubmit,
}: {
  record?: Route;
  logicalModels: LogicalModel[];
  busy: boolean;
  error?: string;
  onSubmit: (input: RouteWriteInput) => void;
}) {
  const { t } = useTranslation('console');
  const [id, setId] = useState(record?.id ?? '');
  const [logicalModelId, setLogicalModelId] = useState(record?.logical_model_id ?? logicalModels[0]?.id ?? '');
  const [protocols, setProtocols] = useState<Set<GatewayProtocol>>(() => new Set(record?.protocols ?? []));
  const [allowLossy, setAllowLossy] = useState(record?.allow_lossy_conversion ?? false);
  const [enabled, setEnabled] = useState(record?.enabled ?? true);
  const [validationError, setValidationError] = useState('');
  const strategy = 'primary_then_weighted_fallback';

  const submit = (event: FormEvent) => {
    event.preventDefault();
    if (!id.trim() || !logicalModelId || protocols.size === 0) {
      setValidationError(t('models.route_form.validate_required'));
      return;
    }
    setValidationError('');
    onSubmit({
      id: id.trim(),
      logical_model_id: logicalModelId,
      protocols: GATEWAY_PROTOCOLS.filter((protocol) => protocols.has(protocol)),
      strategy,
      allow_lossy_conversion: allowLossy,
      enabled,
    });
  };

  return (
    <form id="route-editor-form" className={styles.page} onSubmit={submit}>
      <FormGrid>
        <TextField label={t('models.field.route_id')} value={id} disabled={Boolean(record) || busy} onChange={(event) => setId(event.target.value)} autoComplete="off" />
        <SelectField
          label={t('models.field.lm')}
          value={logicalModelId}
          disabled={busy}
          data={logicalModels.map((model) => ({ value: model.id, label: `${model.display_name} · ${model.id}` }))}
          onChange={setLogicalModelId}
        />
        <TextField label={t('models.field.strategy')} value={strategy} readOnly disabled />
        <div className={styles.field}><label>{t('models.route_form.selection_label')}</label><StatusPill tone="accent">{t('models.state.strategy_fixed')}</StatusPill></div>
        <div className={styles.fullWidth}>
          <span className={styles.fieldLabel}>{t('models.field.protocols')}</span>
          <div className={styles.protocolChecks}>{GATEWAY_PROTOCOLS.map((protocol) => (
            <CheckboxField
              key={protocol}
              checked={protocols.has(protocol)}
              disabled={busy}
              label={PROTOCOL_LABELS[protocol]}
              onChange={(checked) => setProtocols((current) => {
                const next = new Set(current);
                if (checked) next.add(protocol); else next.delete(protocol);
                return next;
              })}
            />
          ))}</div>
        </div>
        <CheckboxField checked={allowLossy} disabled={busy} onChange={setAllowLossy} label={t('models.field.lossy')} hint={t('models.route_form.lossy_hint')} />
        <CheckboxField checked={enabled} disabled={busy} onChange={setEnabled} label={t('models.field.enable_route')} />
      </FormGrid>
      <FormError message={validationError || error} />
    </form>
  );
}
