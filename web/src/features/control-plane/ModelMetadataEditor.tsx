import type {
  MetadataSource,
  ModelMetadataField,
  ModelMetadataValues,
} from '@/admin-api';
import { MODEL_METADATA_FIELDS } from '@/admin-api';
import { SelectField, StatusPill, TextField } from './shared';
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

const METADATA_LABELS: Record<ModelMetadataField, string> = {
  logical_model_name: '建议逻辑模型名',
  display_name: '显示名称',
  context_window: 'Context window',
  max_input_tokens: 'Max input tokens',
  max_output_tokens: 'Max output tokens',
  input_modalities: '输入模态',
  output_modalities: '输出模态',
  tools: 'Tools',
  thinking: 'Thinking',
  web_search: 'Web Search',
  structured_output: 'Structured Output',
  streaming: 'Streaming',
  usage: 'Usage',
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
  return (
    <span className={styles.metadataFieldLabel}>
      <span>{METADATA_LABELS[field]}</span>
      <StatusPill tone={source === 'user' ? 'accent' : source === 'preset' ? 'success' : source === 'upstream' ? 'warning' : 'muted'}>
        {source ?? 'unknown'}
      </StatusPill>
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
  return (
    <div className={styles.formGrid}>
      {(['logical_model_name', 'display_name'] as const).map((field) => (
        <div key={field} className={styles.metadataFieldWrap}>
          <FieldLabel field={field} source={fieldSources[field]} />
          <TextField
            label=""
            aria-label={METADATA_LABELS[field]}
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
            aria-label={METADATA_LABELS[field]}
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
            aria-label={METADATA_LABELS[field]}
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
            aria-label={METADATA_LABELS[field]}
            value={draft[field] || 'unknown'}
            disabled={disabled}
            onChange={(event) => onChange(field, event.target.value)}
            className={styles.metadataField}
          >
            <option value="unknown">unknown</option>
            <option value="supported">supported</option>
            <option value="unsupported">unsupported</option>
          </SelectField>
        </div>
      ))}
    </div>
  );
}
