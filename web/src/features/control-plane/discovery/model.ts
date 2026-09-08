import type {
  Account,
  DiscoveryDiff,
  LatestDiscovery,
  ProviderPresetDefinition,
  Source,
  SourceModel
} from '@/admin-api';

export interface DiscoveryContext {
  sources: Source[];
  accounts: Account[];
}

export interface DiscoveryView {
  latest: LatestDiscovery | null;
  models: SourceModel[];
}

export const emptyDiff = (): DiscoveryDiff => ({ added: [], changed: [], missing: [] });

export const sourceDiscoveryDefinition = (source?: Source) => {
  if (!source?.provider_preset_snapshot || typeof source.provider_preset_snapshot !== 'object') return undefined;
  const definition = source.provider_preset_snapshot as Partial<ProviderPresetDefinition>;
  return definition.discovery;
};

export const statusTone = (status: string) => {
  if (status === 'succeeded' || status === 'confirmed' || status === 'available') return 'success' as const;
  if (status === 'failed' || status === 'unavailable') return 'danger' as const;
  if (status === 'unsupported' || status === 'pending') return 'warning' as const;
  return 'accent' as const;
};

export const metadataSummary = (model: SourceModel): string => {
  const displayName = typeof model.metadata.display_name === 'string' ? model.metadata.display_name : '';
  const logicalName = typeof model.metadata.logical_model_name === 'string' ? model.metadata.logical_model_name : '';
  return displayName || logicalName;
};

export const metadataSourcesSummary = (model: SourceModel, sourceLabel: (source: string) => string): string => {
  const counts = new Map<string, number>();
  for (const source of Object.values(model.field_sources)) {
    if (source) counts.set(source, (counts.get(source) ?? 0) + 1);
  }
  const parts = [...counts.entries()].map(([source, count]) => `${sourceLabel(source)} ${count}`);
  return parts.length > 0 ? parts.join(' · ') : sourceLabel('unknown');
};
