import type {
  ModelMetadataField,
  ModelMetadataValues
} from '@/admin-api';
import { MODEL_METADATA_FIELDS } from '@/admin-api';

export type MetadataDraft = Record<ModelMetadataField, string>;

export const FEATURE_FIELDS = [
  'tools',
  'thinking',
  'web_search',
  'structured_output',
  'streaming',
  'usage',
] as const;

export const NUMBER_FIELDS = ['context_window', 'max_input_tokens', 'max_output_tokens'] as const;

export const MODALITY_FIELDS = ['input_modalities', 'output_modalities'] as const;

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
