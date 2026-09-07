import { SelectField, TextField } from '@/components/ui/FormField';
import { StatusPill } from '@/components/ui/StatusPill';
import type {
  MetadataSource,
  ModelMetadataField,
  ModelMetadataValues,
} from '@/admin-api';
import { MODEL_METADATA_FIELDS } from '@/admin-api';
import { useTranslation } from 'react-i18next';
import {} from './shared';
import styles from './ControlPlane.module.scss';

export type MetadataDraft = Record<ModelMetadataField, string>;

const FEATURE_FIELDS = [
  'tools',
  'thinking',
  'web_search',
  'structured_output',
  'streaming',
  'usage',
] as const;

const NUMBER_FIELDS = ['context_window', 'max_input_tokens', 'max_output_tokens'] as const;
const MODALITY_FIELDS = ['input_modalities', 'output_modalities'] as const;

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

const toDraftValue = (field: ModelMetadataField, value: unknown): string => {
  if (value === null || value === undefined) {
    return FEATURE_FIELDS.includes(field as typeof FEATURE_FIELDS[number]) ? 'unknown' : '';
  }
  if (Array.isArray(value)) return value.filter((item): item is string => typeof item === 'string').join(', ');
  return String(value);
};

export const createMetadataDraft = (values: ModelMetadataValues = {}): MetadataDraft => (
  Object.fromEntries(MODEL_METADATA_FIELDS.map((field) => [field, toDraftValue(field, values[field])])) as MetadataDraft
);

const parseDraftValue = (field: ModelMetadataField, value: string): unknown => {
  if (NUMBER_FIELDS.includes(field as typeof NUMBER_FIELDS[number])) {
    if (!value.trim()) return null;
    return Number(value);
  }
  if (MODALITY_FIELDS.includes(field as typeof MODALITY_FIELDS[number])) {
    const values = value.split(',').map((item) => item.trim()).filter(Boolean);
    return values.length > 0 ? values : null;
  }
  if (FEATURE_FIELDS.includes(field as typeof FEATURE_FIELDS[number])) return value;
  return value.trim() || null;
};

export const metadataFromDraft = (
  draft: MetadataDraft,
  fields: Iterable<ModelMetadataField> = MODEL_METADATA_FIELDS,
): ModelMetadataValues => {
  const values: ModelMetadataValues = {};
  for (const field of fields) values[field] = parseDraftValue(field, draft[field]);
  return values;
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
            onChange={(event) => onChange(field, event.target.value)}
            className={styles.metadataField}
          >
            <option value="unknown">{t('settings.metadata.feature.unknown')}</option>
            <option value="supported">{t('settings.metadata.feature.supported')}</option>
            <option value="unsupported">{t('settings.metadata.feature.unsupported')}</option>
          </SelectField>
        </div>
      ))}
    </div>
  );
}
