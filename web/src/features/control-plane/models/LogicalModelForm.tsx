import type {
  CatalogStatus,
  LogicalModel,
  LogicalModelWriteInput,
  MetadataSource,
  ModelMetadataField
} from '@/admin-api';
import { SelectField, TextField } from '@/components/ui/FormField';
import styles from '@/features/control-plane/ControlPlane.module.scss';
import { ModelMetadataFields } from '@/features/control-plane/ModelMetadataEditor';
import { createMetadataDraft, metadataFromDraft, type MetadataDraft } from '@/features/control-plane/metadata';
import { statusOptions } from '@/features/control-plane/models/catalog';
import { CheckboxField, DrawerSection, FormError, FormGrid } from '@/features/control-plane/shared';
import { useState, type FormEvent } from 'react';
import { useTranslation } from 'react-i18next';

export function LogicalModelForm({
  record,
  busy,
  error,
  onSubmit,
}: {
  record?: LogicalModel;
  busy: boolean;
  error?: string;
  onSubmit: (input: LogicalModelWriteInput) => void;
}) {
  const { t } = useTranslation('console');
  const [id, setId] = useState(record?.id ?? '');
  const [publicName, setPublicName] = useState(record?.public_name ?? '');
  const [displayName, setDisplayName] = useState(record?.display_name ?? '');
  const [status, setStatus] = useState<CatalogStatus>(record?.status ?? 'pending');
  const [enabled, setEnabled] = useState(record?.enabled ?? true);
  const [draft, setDraft] = useState<MetadataDraft>(() => createMetadataDraft(record?.metadata));
  const [dirtyFields, setDirtyFields] = useState<Set<ModelMetadataField>>(() => new Set());
  const [validationError, setValidationError] = useState('');

  const changeMetadata = (field: ModelMetadataField, value: string) => {
    setDraft((current) => ({ ...current, [field]: value }));
    setDirtyFields((current) => new Set(current).add(field));
  };

  const submit = (event: FormEvent) => {
    event.preventDefault();
    if (!id.trim() || !publicName.trim() || !displayName.trim()) {
      setValidationError(t('models.lm_form.validate_required'));
      return;
    }
    const changedMetadata = metadataFromDraft(draft, dirtyFields);
    const metadata = { ...(record?.metadata ?? {}), ...changedMetadata };
    const fieldSources: Partial<Record<ModelMetadataField, MetadataSource>> = {
      ...(record?.field_sources ?? {}),
    };
    for (const field of dirtyFields) fieldSources[field] = 'user';
    const invalidNumber = (['context_window', 'max_input_tokens', 'max_output_tokens'] as const)
      .some((field) => metadata[field] !== undefined && metadata[field] !== null
        && (!Number.isFinite(metadata[field] as number) || (metadata[field] as number) <= 0));
    if (invalidNumber) {
      setValidationError(t('models.lm_form.validate_tokens'));
      return;
    }
    setValidationError('');
    onSubmit({
      id: id.trim(),
      public_name: publicName.trim(),
      display_name: displayName.trim(),
      status,
      metadata,
      field_sources: fieldSources,
      enabled,
    });
  };

  return (
    <form id="logical-model-editor-form" className={styles.page} onSubmit={submit}>
      <FormGrid>
        <TextField label={t('models.field.lm_id')} value={id} disabled={Boolean(record) || busy} onChange={(event) => setId(event.target.value)} autoComplete="off" />
        <TextField label={t('models.field.public_name')} value={publicName} disabled={busy} onChange={(event) => setPublicName(event.target.value)} autoComplete="off" />
        <TextField label={t('models.field.display_name')} value={displayName} disabled={busy} onChange={(event) => setDisplayName(event.target.value)} autoComplete="off" />
        <SelectField label={t('models.field.catalog_status')} value={status} disabled={busy} onChange={(event) => setStatus(event.target.value as CatalogStatus)}>
          {statusOptions(record).map((option) => <option key={option} value={option}>{t(`values.status.${option}`)}</option>)}
        </SelectField>
        <div className={styles.fullWidth}><CheckboxField checked={enabled} disabled={busy} onChange={setEnabled} label={t('models.field.enable_lm')} /></div>
      </FormGrid>
      <DrawerSection title={t('models.field.metadata')}>
        <ModelMetadataFields draft={draft} fieldSources={record?.field_sources} disabled={busy} onChange={changeMetadata} />
      </DrawerSection>
      <FormError message={validationError || error} />
    </form>
  );
}
