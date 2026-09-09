import { FilterPanel } from '@/components/ui/FilterPanel';
import { TextField, SelectField } from '@/components/ui/FormField';
import { Button } from '@/components/ui/Button';
import { RemoteFilterField } from '@/components/ui/RemoteFilterField';
import { GATEWAY_PROTOCOLS } from '@/admin-api';
import type { GatewayUsageClient, UsageOptionField } from '@/gateway-usage/client';
import { PROTOCOL_LABELS } from '@/lib/protocols';
import styles from '@/features/usage/Usage.module.scss';
import { type GatewayUsageFilters } from '@/gateway-usage';
import {
  countActiveAdvancedFilters,
  isValidTimeRange,
  resolveFilterWindow,
} from '@/gateway-usage/filterState';
import type { UsageRelativePreset } from '@/gateway-usage/types';
import { useCallback, useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';

const toLocalInputValue = (iso: string): string => {
  const date = new Date(iso);
  if (!Number.isFinite(date.getTime())) return '';
  const offsetMs = date.getTimezoneOffset() * 60 * 1000;
  return new Date(date.getTime() - offsetMs).toISOString().slice(0, 16);
};

const fromLocalInputValue = (value: string): string => {
  const date = new Date(value);
  return Number.isFinite(date.getTime()) ? date.toISOString() : '';
};

const TIME_PRESETS: Array<{ key: UsageRelativePreset; labelKey: string }> = [
  { key: 'today', labelKey: 'usage.filter.preset_today' },
  { key: 'yesterday', labelKey: 'usage.filter.preset_yesterday' },
  { key: '24h', labelKey: 'usage.filter.preset_24h' },
  { key: '7d', labelKey: 'usage.filter.preset_7d' },
  { key: '30d', labelKey: 'usage.filter.preset_30d' },
];

interface FilterBarProps {
  client: Pick<GatewayUsageClient, 'filterOptions'>;
  authGeneration?: number;
  refreshRevision?: number;
  draft: GatewayUsageFilters;
  onChange: (value: GatewayUsageFilters) => void;
  onApply: () => void;
  onPresetSelect: (preset: UsageRelativePreset) => void;
  onReset: () => void;
  loading: boolean;
}

function UsageOption({ client, field, from, to, contextKey, ...props }: {
  client: FilterBarProps['client']; field: UsageOptionField; from: string; to: string;
  contextKey: string; label: string; value: string; onChange: (value: string) => void; inputMode?: 'numeric';
}) {
  const loadOptions = useCallback((search: string, signal: AbortSignal) => {
    if (!isValidTimeRange(from, to)) return Promise.reject(new Error('invalid_time_range'));
    return client.filterOptions(field, { from, to }, search, signal);
  }, [client, field, from, to]);
  return <RemoteFilterField {...props} className={styles.filterField} loadOptions={loadOptions} contextKey={contextKey} />;
}

export function FilterBar({ client, authGeneration = 0, refreshRevision = 0, draft, onChange, onApply, onPresetSelect, onReset, loading }: FilterBarProps) {
  const { t } = useTranslation('console');
  const [showAdvanced, setShowAdvanced] = useState(false);
  const { from, to, timeMode, relativePreset } = draft;
  const optionScope = useMemo(() => ({
    ...resolveFilterWindow({ from, to, timeMode, relativePreset }),
    contextKey: JSON.stringify([authGeneration, refreshRevision]),
  }), [from, to, timeMode, relativePreset, authGeneration, refreshRevision]);
  const optionProps = { client, from: optionScope.from, to: optionScope.to, contextKey: optionScope.contextKey };
  const protocols = [{ value: '', label: t('common.all') }, ...GATEWAY_PROTOCOLS.map((protocol) => ({ value: protocol, label: PROTOCOL_LABELS[protocol] }))];
  const advancedCount = countActiveAdvancedFilters(draft);
  const update = (field: keyof GatewayUsageFilters, value: string) => onChange({
    ...draft,
    [field]: value || undefined,
  });
  const updateAbsoluteTime = (field: 'from' | 'to', value: string) => onChange({
    ...draft,
    timeMode: 'absolute',
    [field]: fromLocalInputValue(value),
  });
  const selectCustom = () => onChange({
    ...draft,
    timeMode: 'absolute',
  });

  return (
    <FilterPanel className={styles.filters} label={t('usage.filter.aria')}>
      <div className={styles.commonFilters}>
        <div className={styles.filterPresets}>
          {TIME_PRESETS.map((preset) => (
            <Button variant={draft.relativePreset === preset.key && draft.timeMode === 'relative' ? 'primary' : 'secondary'}
              key={preset.key}
              type="button"
              aria-pressed={draft.relativePreset === preset.key && draft.timeMode === 'relative'}
              data-active={draft.relativePreset === preset.key && draft.timeMode === 'relative'}
              onClick={() => onPresetSelect(preset.key)}
            >
              {t(preset.labelKey)}
            </Button>
          ))}
          <Button variant={draft.timeMode === 'absolute' ? 'primary' : 'secondary'}
            type="button"
            aria-pressed={draft.timeMode === 'absolute'}
            data-active={draft.timeMode === 'absolute'}
            onClick={selectCustom}
          >
            {t('usage.filter.preset_custom')}
          </Button>
        </div>
        {draft.timeMode === 'absolute' && (
          <div className={styles.timeRange}>
            <TextField className={styles.filterField} label={t('usage.filter.from')} type="datetime-local" value={toLocalInputValue(draft.from)} onChange={(event) => updateAbsoluteTime('from', event.target.value)} />
            <TextField className={styles.filterField} label={t('usage.filter.to')} type="datetime-local" value={toLocalInputValue(draft.to)} onChange={(event) => updateAbsoluteTime('to', event.target.value)} />
          </div>
        )}
        <UsageOption {...optionProps} field="logical_model" label={t('usage.field.logical_model')} value={draft.logicalModel ?? ''} onChange={(value) => update('logicalModel', value)} />
        <UsageOption {...optionProps} field="provider" label={t('usage.field.provider')} value={draft.provider ?? ''} onChange={(value) => update('provider', value)} />
        <SelectField
          className={styles.filterField}
          label={t('usage.field.status')}
          value={draft.status ?? ''}
          data={[
            { value: '', label: t('common.all') },
            { value: 'success', label: t('usage.filter.status_success') },
            { value: 'failure', label: t('usage.filter.status_failure') },
          ]}
          onChange={(value) => update('status', value)}
        />
        <Button variant="ghost" aria-expanded={showAdvanced} onClick={() => setShowAdvanced(!showAdvanced)}>
          {t('usage.filter.advanced')}{advancedCount > 0 ? ` (${advancedCount})` : ''}
        </Button>
        <Button variant="ghost" onClick={onReset}>{t('usage.filter.reset')}</Button>
        <Button variant="secondary" onClick={onApply} loading={loading}>{t('usage.filter.apply')}</Button>
      </div>
      {showAdvanced && (
        <div className={styles.advancedFilters}>
          <UsageOption {...optionProps} field="upstream_model" label={t('usage.field.upstream_model')} value={draft.upstreamModel ?? ''} onChange={(value) => update('upstreamModel', value)} />
          <UsageOption {...optionProps} field="source_id" label={t('usage.field.source_id')} value={draft.sourceId ?? ''} onChange={(value) => update('sourceId', value)} />
          <UsageOption {...optionProps} field="account" label={t('usage.field.account')} value={draft.account ?? ''} onChange={(value) => update('account', value)} />
          <UsageOption {...optionProps} field="client_source" label={t('usage.field.client_source')} value={draft.clientSource ?? ''} onChange={(value) => update('clientSource', value)} />
          <SelectField className={styles.filterField} label={t('usage.field.protocol_in')} value={draft.protocolIn ?? ''} data={protocols} onChange={(value) => update('protocolIn', value)} />
          <SelectField className={styles.filterField} label={t('usage.field.protocol_upstream')} value={draft.protocolUpstream ?? ''} data={protocols} onChange={(value) => update('protocolUpstream', value)} />
          <UsageOption {...optionProps} field="virtual_key" label={t('usage.field.virtual_key_id')} inputMode="numeric" value={draft.virtualKey ?? ''} onChange={(value) => update('virtualKey', value)} />
          <SelectField
            className={styles.filterField}
            label={t('usage.field.usage_source')}
            value={draft.usageSource ?? ''}
            data={[
              { value: '', label: t('common.all') },
              { value: 'upstream', label: t('usage.usage_source.upstream') },
              { value: 'parsed', label: t('usage.usage_source.parsed') },
              { value: 'estimated', label: t('usage.usage_source.estimated') },
              { value: 'missing', label: t('usage.usage_source.missing') },
            ]}
            onChange={(value) => update('usageSource', value)}
          />
        </div>
      )}
    </FilterPanel>
  );
}
