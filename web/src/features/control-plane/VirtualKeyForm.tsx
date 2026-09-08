import { TextAreaField, TextField } from '@/components/ui/FormField';
import styles from '@/features/control-plane/ControlPlane.module.scss';
import { FormError, FormGrid } from '@/features/control-plane/shared';
import { useState, type FormEvent } from 'react';
import { useTranslation } from 'react-i18next';

export function VirtualKeyForm({
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
