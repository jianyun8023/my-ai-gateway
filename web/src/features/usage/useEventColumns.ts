import { readLocalPreference, writeLocalPreference } from '@/lib/browserStorage';
import { useCallback, useState } from 'react';
import { DEFAULT_VISIBLE_COLUMNS, normalizeVisibleEventColumns, type EventColumn } from './eventColumns';

// Bump the suffix when a changed default column set must reset saved choices.
const COLUMNS_STORAGE_KEY = 'my-ai-gateway-usage-event-columns-v4';

function loadVisibleColumns(): EventColumn[] {
  try {
    return normalizeVisibleEventColumns(JSON.parse(readLocalPreference(COLUMNS_STORAGE_KEY)));
  } catch {
    return DEFAULT_VISIBLE_COLUMNS;
  }
}

export function useEventColumns() {
  const [visibleColumns, setVisibleColumns] = useState(loadVisibleColumns);
  const changeVisibleColumns = useCallback((columns: EventColumn[]) => {
    const normalized = normalizeVisibleEventColumns(columns);
    writeLocalPreference(COLUMNS_STORAGE_KEY, normalized);
    setVisibleColumns(normalized);
  }, []);
  return { visibleColumns, changeVisibleColumns };
}
