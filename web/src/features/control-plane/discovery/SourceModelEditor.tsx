import type {
  ModelMetadataField,
  ModelMetadataValues,
  SourceModel
} from '@/admin-api';
import { StatusPill } from '@/components/ui/StatusPill';
import styles from '@/features/control-plane/ControlPlane.module.scss';
import { ModelMetadataFields } from '@/features/control-plane/ModelMetadataEditor';
import { statusTone } from '@/features/control-plane/discovery/model';
import { createMetadataDraft, metadataFromDraft, type MetadataDraft } from '@/features/control-plane/metadata';
import { FormError } from '@/features/control-plane/shared';
import { useState, type FormEvent } from 'react';
import { useTranslation } from 'react-i18next';

export function SourceModelEditor({
  model,
  busy,
  error,
  onSubmit,
}: {
  model: SourceModel;
  busy: boolean;
  error?: string;
  onSubmit: (metadata: ModelMetadataValues) => void;
}) {
  const { t } = useTranslation('console');
  const [draft, setDraft] = useState<MetadataDraft>(() => createMetadataDraft(model.metadata));
  const [dirty, setDirty] = useState<Set<ModelMetadataField>>(() => new Set());
  const [validationError, setValidationError] = useState('');

  const change = (field: ModelMetadataField, value: string) => {
    setDraft((current) => ({ ...current, [field]: value }));
    setDirty((current) => new Set(current).add(field));
  };

  const submit = (event: FormEvent) => {
    event.preventDefault();
    if (dirty.size === 0) {
      setValidationError(t('discovery.no_field_changes'));
      return;
    }
    const metadata = metadataFromDraft(draft, dirty);
    const invalidNumber = (['context_window', 'max_input_tokens', 'max_output_tokens'] as const)
      .some((field) => metadata[field] !== undefined && metadata[field] !== null
        && (!Number.isFinite(metadata[field] as number) || (metadata[field] as number) <= 0));
    if (invalidNumber) {
      setValidationError(t('discovery.validate_tokens'));
      return;
    }
    setValidationError('');
    onSubmit(metadata);
  };

  return (
    <form id="source-model-editor-form" className={styles.page} onSubmit={submit}>
      <div className={styles.modelIdentity}>
        <code>{model.upstream_model_id}</code>
        <span><StatusPill tone={statusTone(model.confirmation_status)}>{t(`discovery.confirm_state.${model.confirmation_status}`)}</StatusPill><StatusPill tone={statusTone(model.availability_status)}>{t(`discovery.availability_state.${model.availability_status}`)}</StatusPill></span>
      </div>
      <ModelMetadataFields draft={draft} fieldSources={model.field_sources} disabled={busy} onChange={change} />
      <FormError message={validationError || error} />
    </form>
  );
}
