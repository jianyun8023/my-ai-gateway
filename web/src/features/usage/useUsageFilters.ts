import { defaultFilters, FILTER_STORAGE_KEY, isValidTimeRange, resolveFilterWindow, safeParseFilters, serializeFiltersForStorage } from '@/gateway-usage/filterState';
import type { GatewayUsageFilters, UsageRelativePreset } from '@/gateway-usage/types';
import { writeLocalPreference } from '@/lib/browserStorage';
import { useState } from 'react';

export function useUsageFilters() {
  const [filters, setFilters] = useState(safeParseFilters);
  const [draft, setDraft] = useState(filters);
  const [invalidRange, setInvalidRange] = useState(false);

  const apply = (next: GatewayUsageFilters) => {
    const resolved = resolveFilterWindow(next);
    if (!isValidTimeRange(resolved.from, resolved.to)) {
      setInvalidRange(true);
      return;
    }
    setInvalidRange(false);
    writeLocalPreference(FILTER_STORAGE_KEY, serializeFiltersForStorage(resolved));
    setFilters(resolved);
    setDraft(resolved);
  };

  return {
    filters, draft, setDraft, invalidRange,
    apply: () => apply(draft),
    selectPreset: (preset: UsageRelativePreset) => apply({ ...draft, timeMode: 'relative', relativePreset: preset }),
    reset: () => apply(defaultFilters()),
  };
}
