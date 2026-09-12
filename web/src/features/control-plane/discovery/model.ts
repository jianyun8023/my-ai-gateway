import type {
  DiscoveryDiff,
  LatestDiscovery,
  SourceModel
} from '@/admin-api';
import type { TFunction } from 'i18next';

const emptyDiff = (): DiscoveryDiff => ({ added: [], changed: [], missing: [] });

export const statusTone = (status: string) => {
  if (status === 'succeeded' || status === 'confirmed' || status === 'available') return 'success' as const;
  if (status === 'failed' || status === 'unavailable') return 'danger' as const;
  if (status === 'unsupported' || status === 'pending') return 'warning' as const;
  return 'accent' as const;
};

/** 单个来源的模型同步汇总，全部来自真实 API 数据（latest run diff + source model 列表）。 */
export interface SourceSyncStats {
  latest: LatestDiscovery | null;
  models: SourceModel[];
  pendingCount: number;
  /** 最近一次运行是否产出可信差异；failed / unsupported / 尚未运行时差异不可判断。 */
  diffAvailable: boolean;
  addedCount: number;
  changedCount: number;
  missingCount: number;
  unchangedCount: number;
  lastSyncAt: string | null;
}

export const buildSyncStats = (latest: LatestDiscovery | null, models: SourceModel[]): SourceSyncStats => {
  const diff = latest?.diff ?? latest?.run.diff ?? emptyDiff();
  const diffAvailable = latest?.run.status === 'succeeded';
  const changedIds = new Set(
    [...diff.added, ...diff.changed, ...diff.missing].map((entry) => entry.upstream_model_id),
  );
  return {
    latest,
    models,
    pendingCount: models.filter((model) => model.confirmation_status === 'pending').length,
    diffAvailable,
    addedCount: diff.added.length,
    changedCount: diff.changed.length,
    missingCount: diff.missing.length,
    unchangedCount: diffAvailable ? models.filter((model) => !changedIds.has(model.upstream_model_id)).length : 0,
    lastSyncAt: latest?.last_discovered_at ?? latest?.run.completed_at ?? null,
  };
};

/** 模型能力摘要：上下文长度 + 已声明的关键能力，只展示真实元数据。 */
export const capabilitySummary = (model: SourceModel, t: TFunction): string => {
  const parts: string[] = [];
  const context = model.metadata.context_window;
  if (typeof context === 'number' && context > 0) parts.push(`${Math.round(context / 1000)}K`);
  for (const feature of ['tools', 'thinking', 'web_search', 'structured_output'] as const) {
    const value = model.metadata[feature];
    if (value === true || value === 'native') parts.push(t(`discovery.feature.${feature}`));
  }
  return parts.join(' · ');
};

/** 最近一次发现中每个模型的变化类型。 */
export type DiscoveryChangeKind = 'added' | 'changed' | 'missing';

export const diffChangeMap = (latest: LatestDiscovery | null): Map<string, { kind: DiscoveryChangeKind; changedFields: string[] }> => {
  const map = new Map<string, { kind: DiscoveryChangeKind; changedFields: string[] }>();
  const diff = latest?.diff ?? latest?.run.diff ?? emptyDiff();
  for (const entry of diff.added) map.set(entry.upstream_model_id, { kind: 'added', changedFields: entry.changed_fields });
  for (const entry of diff.changed) map.set(entry.upstream_model_id, { kind: 'changed', changedFields: entry.changed_fields });
  for (const entry of diff.missing) map.set(entry.upstream_model_id, { kind: 'missing', changedFields: entry.changed_fields });
  return map;
};
