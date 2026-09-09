import { AdminClient } from '@/admin-api/client';
import { Button } from '@/components/ui/Button';
import { LoadingState } from '@/components/ui/LoadingState';
import { Notice } from '@/components/ui/Notice';
import { SegmentedTabs } from '@/components/ui/SegmentedTabs';
import { COLUMNS_STORAGE_KEY, loadVisibleColumns, normalizeVisibleEventColumns, type EventColumn } from '@/features/usage/eventColumns';
import type { TrendMetric } from '@/features/usage/model';
import styles from '@/features/usage/Usage.module.scss';
import { Analysis } from '@/features/usage/UsageAnalysis';
import { EventsTable } from '@/features/usage/UsageEvents';
import { FilterBar } from '@/features/usage/UsageFilters';
import { Overview } from '@/features/usage/UsageOverview';
import { useUsageData } from '@/features/usage/useUsageData';
import { useUsageFilters } from '@/features/usage/useUsageFilters';
import { GatewayUsageClient } from '@/gateway-usage/client';
import { useLocalizedApiError } from '@/hooks/useLocalizedApiError';
import { writeLocalPreference } from '@/lib/browserStorage';
import type { GatewayUsageTab } from '@/lib/consoleNavigation';
import { useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';

interface GatewayUsagePageProps {
  activeTab: GatewayUsageTab;
  getAdminKey: () => string;
  authGeneration: number;
  refreshRevision: number;
  onLoadingChange?: (loading: boolean) => void;
}

export function GatewayUsagePage({ activeTab, getAdminKey, authGeneration, refreshRevision, onLoadingChange }: GatewayUsagePageProps) {
  const { t } = useTranslation('console');
  const localizeError = useLocalizedApiError();
  const client = useMemo(() => new GatewayUsageClient(new AdminClient({ getAdminKey })), [getAdminKey]);
  const filterState = useUsageFilters();
  const [visibleColumns, setVisibleColumns] = useState<EventColumn[]>(loadVisibleColumns);
  const [trendMetric, setTrendMetric] = useState<TrendMetric>('composition');
  const [granularity, setGranularity] = useState<'auto' | 'hour' | 'day'>('auto');
  const data = useUsageData({ client, filters: filterState.filters, activeTab, granularity, authGeneration, refreshRevision, onLoadingChange });
  const { loading, overview, analysisSummary } = data;
  const localizeUsageError = (entry?: { cause: unknown; messageKey: string }) => {
    if (!entry) return undefined;
    const mapped = localizeError(entry.cause);
    return mapped === t('errors.unknown') ? t(entry.messageKey) : mapped;
  };
  const error = filterState.invalidRange ? t('usage.error.invalid_range') : localizeUsageError(data.error);
  const loadMoreError = localizeUsageError(data.loadMoreError);
  const exportError = localizeUsageError(data.exportError);
  const hasData = activeTab === 'overview'
    ? overview !== undefined
    : activeTab === 'analysis'
      ? analysisSummary !== undefined
      : data.eventPage !== undefined;
  const changeVisibleColumns = (columns: EventColumn[]) => {
    const normalized = normalizeVisibleEventColumns(columns);
    writeLocalPreference(COLUMNS_STORAGE_KEY, normalized);
    setVisibleColumns(normalized);
  };

  return (
    <section className={styles.content} data-od-id={`page-${activeTab}`}>
      <FilterBar
        client={client}
        authGeneration={authGeneration}
        refreshRevision={refreshRevision}
        draft={filterState.draft}
        onChange={filterState.setDraft}
        onApply={filterState.apply}
        onPresetSelect={filterState.selectPreset}
        onReset={filterState.reset}
        loading={loading || data.refreshing}
      />
      {activeTab === 'overview' && (
        <div className={styles.granularityBar}>
          <span>{t('usage.granularity.label')}</span>
          <SegmentedTabs mode="group" label={t('usage.granularity.label')} value={granularity} onChange={setGranularity} options={[
            { value: 'auto', label: t('usage.granularity.auto') },
            { value: 'hour', label: t('usage.granularity.hour') },
            { value: 'day', label: t('usage.granularity.day') },
          ]} />
        </div>
      )}
      {error && <Notice action={!filterState.invalidRange && data.error ? <Button size="sm" variant="secondary" onClick={data.reload}>{t('common.retry')}</Button> : undefined}>{error}</Notice>}
      {data.refreshing && <LoadingState layout="inline" label={t('usage.page.refreshing')} />}
      {loading && !hasData ? <LoadingState label={t('usage.page.loading')} /> : hasData ? (
        activeTab === 'overview'
          ? overview && <Overview data={overview} metric={trendMetric} onMetricChange={setTrendMetric} />
          : activeTab === 'analysis'
            ? analysisSummary && <Analysis breakdowns={data.breakdowns ?? {}} summary={analysisSummary} />
            : <EventsTable events={data.eventPage?.events ?? []} hasMore={data.eventPage?.hasMore ?? false}
              loadingMore={data.loadingMore} loadMoreError={loadMoreError} onLoadMore={data.loadMore} onRetryLoadMore={data.retryLoadMore}
              visibleColumns={visibleColumns} onVisibleColumnsChange={changeVisibleColumns}
              onExport={data.exportEvents} exportingFormat={data.exportingFormat} exportError={exportError} onRetryExport={data.retryExport}
              client={client} />
      ) : null}
    </section>
  );
}
