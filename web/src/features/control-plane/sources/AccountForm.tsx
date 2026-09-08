import type {
  Account,
  AccountWriteInput,
  Source
} from '@/admin-api';
import { SelectField, TextField } from '@/components/ui/FormField';
import { StatusPill } from '@/components/ui/StatusPill';
import styles from '@/features/control-plane/ControlPlane.module.scss';
import { CheckboxField, FormError, FormGrid } from '@/features/control-plane/shared';
import { credentialKey } from '@/features/control-plane/sources/presentation';
import { useState, type FormEvent } from 'react';
import { useTranslation } from 'react-i18next';

export function AccountForm({
  record,
  sources,
  busy,
  error,
  onSubmit,
}: {
  record?: Account;
  sources: Source[];
  busy: boolean;
  error?: string;
  onSubmit: (input: AccountWriteInput) => void;
}) {
  const { t } = useTranslation('console');
  const [id, setId] = useState(record?.id ?? '');
  const [sourceId, setSourceId] = useState(record?.source_id ?? sources[0]?.id ?? '');
  const [displayName, setDisplayName] = useState(record?.display_name ?? '');
  // Existing credential references are intentionally not echoed into the DOM.
  // Editing an account requires explicitly providing the environment variable again.
  const [credentialEnv, setCredentialEnv] = useState('');
  const [weight, setWeight] = useState(record?.weight ?? 100);
  const [enabled, setEnabled] = useState(record?.enabled ?? true);
  const [validationError, setValidationError] = useState('');

  const submit = (event: FormEvent) => {
    event.preventDefault();
    if (!id.trim() || !sourceId || !displayName.trim() || !credentialEnv.trim()) {
      setValidationError(t('sources.form.validate_required_account'));
      return;
    }
    if (!Number.isInteger(weight) || weight <= 0) {
      setValidationError(t('sources.form.validate_weight'));
      return;
    }
    setValidationError('');
    onSubmit({
      id: id.trim(),
      source_id: sourceId,
      display_name: displayName.trim(),
      credential_env: credentialEnv.trim(),
      credential_ciphertext: null,
      enabled,
      weight,
    });
  };

  return (
    <form id="account-editor-form" className={styles.page} onSubmit={submit}>
      <FormGrid>
        <TextField label={t('sources.field.account_id')} value={id} disabled={Boolean(record) || busy} onChange={(event) => setId(event.target.value)} autoComplete="off" />
        <TextField label={t('sources.field.display_name')} value={displayName} disabled={busy} onChange={(event) => setDisplayName(event.target.value)} autoComplete="off" />
        <SelectField label={t('sources.field.source')} value={sourceId} disabled={busy} onChange={(event) => setSourceId(event.target.value)}>
          {sources.map((source) => <option key={source.id} value={source.id}>{source.display_name} · {source.id}</option>)}
        </SelectField>
        <TextField label={t('sources.field.credential_env')} hint={record ? t('sources.form.credential_env_hint_no_echo') : t('sources.form.credential_env_hint_name_only')} value={credentialEnv} disabled={busy} onChange={(event) => setCredentialEnv(event.target.value)} autoComplete="off" spellCheck={false} />
        <TextField label={t('sources.field.fallback_weight')} type="number" min={1} step={1} value={weight} disabled={busy} onChange={(event) => setWeight(Number(event.target.value))} />
        <div className={styles.field}>
          <label>{t('sources.field.credential_status')}</label>
          <StatusPill tone={record?.credential_configured ? 'success' : 'muted'}>{record ? t(credentialKey(record)) : t('sources.form.verify_on_submit')}</StatusPill>
        </div>
        <div className={styles.fullWidth}>
          <CheckboxField checked={enabled} disabled={busy} onChange={setEnabled} label={t('sources.field.enable_account')} />
        </div>
      </FormGrid>
      <FormError message={validationError || error} />
    </form>
  );
}
