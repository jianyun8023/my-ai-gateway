import type {
  MetadataSource,
  ModelMetadataField
} from '@/admin-api';
import { SelectField, TextField } from '@/components/ui/FormField';
import { StatusPill } from '@/components/ui/StatusPill';
import styles from '@/features/control-plane/ControlPlane.module.scss';
import { FEATURE_FIELDS, MODALITY_FIELDS, NUMBER_FIELDS, type MetadataDraft } from '@/features/control-plane/metadata';
import { useTranslation } from 'react-i18next';

const METADATA_LABEL_KEYS: Record<ModelMetadataField, string> = {
  logical_model_name: 'settings.metadata.field.logical_model_name',
  display_name: 'settings.metadata.field.display_name',
  context_window: 'settings.metadata.field.context_window',
  max_input_tokens: 'settings.metadata.field.max_input_tokens',
  max_output_tokens: 'settings.metadata.field.max_output_tokens',
  input_modalities: 'settings.metadata.field.input_modalities',
  output_modalities: 'settings.metadata.field.output_modalities',
  tools: 'settings.metadata.field.tools',
  thinking: 'settings.metadata.field.thinking',
  web_search: 'settings.metadata.field.web_search',
  structured_output: 'settings.metadata.field.structured_output',
  streaming: 'settings.metadata.field.streaming',
  usage: 'settings.metadata.field.usage',
};

function FieldLabel({ field, source }: { field: ModelMetadataField; source?: MetadataSource }) {
  const { t } = useTranslation('console');
  return (
    <span className={styles.metadataFieldLabel}>
      <span>{t(METADATA_LABEL_KEYS[field])}</span>
      {source && (
        <StatusPill tone={source === 'user' ? 'accent' : source === 'preset' ? 'success' : source === 'upstream' ? 'warning' : 'muted'}>
          {t(`models.metadata_source.${source}`, { defaultValue: source })}
        </StatusPill>
      )}
    </span>
  );
}

export function ModelMetadataFields({
  draft,
  fieldSources = {},
  disabled = false,
  onChange,
}: {
  draft: MetadataDraft;
  fieldSources?: Partial<Record<ModelMetadataField, MetadataSource>>;
  disabled?: boolean;
  onChange: (field: ModelMetadataField, value: string) => void;
}) {
  const { t } = useTranslation('console');
  const fieldLabel = (field: ModelMetadataField) => t(METADATA_LABEL_KEYS[field]);
  return (
    <div className={styles.formGrid}>
      {(['logical_model_name', 'display_name'] as const).map((field) => (
        <div key={field} className={styles.metadataFieldWrap}>
          <FieldLabel field={field} source={fieldSources[field]} />
          <TextField
            label=""
            aria-label={fieldLabel(field)}
            value={draft[field]}
            disabled={disabled}
            onChange={(event) => onChange(field, event.target.value)}
            className={styles.metadataField}
          />
        </div>
      ))}

      {NUMBER_FIELDS.map((field) => (
        <div key={field} className={styles.metadataFieldWrap}>
          <FieldLabel field={field} source={fieldSources[field]} />
          <TextField
            label=""
            aria-label={fieldLabel(field)}
            type="number"
            min={1}
            step={1}
            value={draft[field]}
            disabled={disabled}
            onChange={(event) => onChange(field, event.target.value)}
            className={styles.metadataField}
          />
        </div>
      ))}

      {MODALITY_FIELDS.map((field) => (
        <div key={field} className={styles.metadataFieldWrap}>
          <FieldLabel field={field} source={fieldSources[field]} />
          <TextField
            label=""
            aria-label={fieldLabel(field)}
            value={draft[field]}
            disabled={disabled}
            onChange={(event) => onChange(field, event.target.value)}
            className={styles.metadataField}
          />
        </div>
      ))}

      {FEATURE_FIELDS.map((field) => (
        <div key={field} className={styles.metadataFieldWrap}>
          <FieldLabel field={field} source={fieldSources[field]} />
          <SelectField
            label=""
            aria-label={fieldLabel(field)}
            value={draft[field] || 'unknown'}
            disabled={disabled}
            data={[
              { value: 'unknown', label: t('settings.metadata.feature.unknown') },
              { value: 'supported', label: t('settings.metadata.feature.supported') },
              { value: 'unsupported', label: t('settings.metadata.feature.unsupported') },
            ]}
            onChange={(value) => onChange(field, value)}
            className={styles.metadataField}
          />
        </div>
      ))}
    </div>
  );
}
