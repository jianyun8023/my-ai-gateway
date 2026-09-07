import { useState, type FormEvent } from 'react';
import { useTranslation } from 'react-i18next';
import type { VirtualKey, VirtualKeyRotateInput } from '@/admin-api';
import { TextAreaField, TextField } from '@/components/ui/FormField';
import { FormError, FormGrid } from './shared';
import styles from './ControlPlane.module.scss';

export function VirtualKeyRotationForm({ target, busy, error, onSubmit }: {
  target: VirtualKey;
  busy: boolean;
  error?: string;
  onSubmit: (input: VirtualKeyRotateInput) => void;
}) {
  const { t } = useTranslation('console');
  const [overlap, setOverlap] = useState('3600');
  const [allowedModels, setAllowedModels] = useState(target.allowed_models.join(', '));
  const [validationError, setValidationError] = useState('');

  const submit = (event: FormEvent) => {
    event.preventDefault();
    if (busy) return;
    const seconds = Number(overlap);
    if (!overlap.trim() || !Number.isInteger(seconds) || seconds < 0 || seconds > 86400) {
      setValidationError(t('settings.overlap_invalid'));
      return;
    }
    setValidationError('');
    onSubmit({
      overlap_secs: seconds,
      allowed_models: [...new Set(allowedModels.split(',').map((model) => model.trim()).filter(Boolean))],
    });
  };

  return (
    <form id="virtual-key-rotation-form" className={styles.page} onSubmit={submit} noValidate>
      <p>{t('settings.rotation_hint')}</p>
      <FormGrid>
        <TextField label={t('settings.overlap_seconds')} hint={t('settings.overlap_hint')} type="number" min={0} max={86400} step={1} required value={overlap} disabled={busy} onChange={(event) => setOverlap(event.target.value)} />
        <TextAreaField label={t('settings.allowed_models')} hint={t('settings.allowed_models_hint')} value={allowedModels} disabled={busy} onChange={(event) => setAllowedModels(event.target.value)} />
      </FormGrid>
      {overlap.trim() && Number(overlap) === 0 && <p role="status">{t('settings.rotation_immediate')}</p>}
      <FormError message={validationError || error} />
    </form>
  );
}
