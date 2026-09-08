import { FilterPanel } from '@/components/ui/FilterPanel';
import { TextField, SelectField } from '@/components/ui/FormField';
import { Button } from '@/components/ui/Button';
import styles from '@/features/usage/Usage.module.scss';
import { type GatewayUsageFilters } from '@/gateway-usage';
import {
  countActiveAdvancedFilters,
} from '@/gateway-usage/filterState';
import type { UsageRelativePreset } from '@/gateway-usage/types';
import { useState } from 'react';
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
  draft: GatewayUsageFilters;
  onChange: (value: GatewayUsageFilters) => void;
  onApply: () => void;
  onPresetSelect: (preset: UsageRelativePreset) => void;
  onReset: () => void;
  loading: boolean;
}

export function FilterBar({ draft, onChange, onApply, onPresetSelect, onReset, loading }: FilterBarProps) {
  const { t } = useTranslation('console');
  const [showAdvanced, setShowAdvanced] = useState(false);
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
        <TextField className={styles.filterField} label={t('usage.field.logical_model')} value={draft.logicalModel ?? ''} onChange={(event) => update('logicalModel', event.target.value)} placeholder={t('common.all')} />
        <TextField className={styles.filterField} label={t('usage.field.provider')} value={draft.provider ?? ''} onChange={(event) => update('provider', event.target.value)} placeholder={t('common.all')} />
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
          <TextField className={styles.filterField} label={t('usage.field.upstream_model')} value={draft.upstreamModel ?? ''} onChange={(event) => update('upstreamModel', event.target.value)} placeholder={t('common.all')} />
          <TextField className={styles.filterField} label={t('usage.field.source_id')} value={draft.sourceId ?? ''} onChange={(event) => update('sourceId', event.target.value)} placeholder={t('common.all')} />
          <TextField className={styles.filterField} label={t('usage.field.account')} value={draft.account ?? ''} onChange={(event) => update('account', event.target.value)} placeholder={t('common.all')} />
          <TextField className={styles.filterField} label={t('usage.field.client_source')} value={draft.clientSource ?? ''} onChange={(event) => update('clientSource', event.target.value)} placeholder={t('common.all')} />
          <TextField className={styles.filterField} label={t('usage.field.protocol_in')} value={draft.protocolIn ?? ''} onChange={(event) => update('protocolIn', event.target.value)} placeholder={t('common.all')} />
          <TextField className={styles.filterField} label={t('usage.field.protocol_upstream')} value={draft.protocolUpstream ?? ''} onChange={(event) => update('protocolUpstream', event.target.value)} placeholder={t('common.all')} />
          <TextField className={styles.filterField} label={t('usage.field.virtual_key_id')} inputMode="numeric" value={draft.virtualKey ?? ''} onChange={(event) => update('virtualKey', event.target.value)} placeholder={t('common.all')} />
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
