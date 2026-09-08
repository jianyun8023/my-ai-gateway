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
    <section className={styles.filters} aria-label={t('usage.filter.aria')}>
      <div className={styles.commonFilters}>
        <div className={styles.filterPresets}>
          {TIME_PRESETS.map((preset) => (
            <button
              key={preset.key}
              type="button"
              aria-pressed={draft.relativePreset === preset.key && draft.timeMode === 'relative'}
              data-active={draft.relativePreset === preset.key && draft.timeMode === 'relative'}
              onClick={() => onPresetSelect(preset.key)}
            >
              {t(preset.labelKey)}
            </button>
          ))}
          <button
            type="button"
            aria-pressed={draft.timeMode === 'absolute'}
            data-active={draft.timeMode === 'absolute'}
            onClick={selectCustom}
          >
            {t('usage.filter.preset_custom')}
          </button>
        </div>
        {draft.timeMode === 'absolute' && (
          <div className={styles.timeRange}>
            <label>{t('usage.filter.from')}<input type="datetime-local" value={toLocalInputValue(draft.from)} onChange={(event) => updateAbsoluteTime('from', event.target.value)} /></label>
            <label>{t('usage.filter.to')}<input type="datetime-local" value={toLocalInputValue(draft.to)} onChange={(event) => updateAbsoluteTime('to', event.target.value)} /></label>
          </div>
        )}
        <label>{t('usage.field.logical_model')}<input value={draft.logicalModel ?? ''} onChange={(event) => update('logicalModel', event.target.value)} placeholder={t('common.all')} /></label>
        <label>{t('usage.field.provider')}<input value={draft.provider ?? ''} onChange={(event) => update('provider', event.target.value)} placeholder={t('common.all')} /></label>
        <label>{t('usage.field.status')}<select value={draft.status ?? ''} onChange={(event) => update('status', event.target.value)}><option value="">{t('common.all')}</option><option value="success">{t('usage.filter.status_success')}</option><option value="failure">{t('usage.filter.status_failure')}</option></select></label>
        <Button variant="ghost" aria-expanded={showAdvanced} onClick={() => setShowAdvanced(!showAdvanced)}>
          {t('usage.filter.advanced')}{advancedCount > 0 ? ` (${advancedCount})` : ''}
        </Button>
        <Button variant="ghost" onClick={onReset}>{t('usage.filter.reset')}</Button>
        <Button variant="secondary" onClick={onApply} loading={loading}>{t('usage.filter.apply')}</Button>
      </div>
      {showAdvanced && (
        <div className={styles.advancedFilters}>
          <label>{t('usage.field.upstream_model')}<input value={draft.upstreamModel ?? ''} onChange={(event) => update('upstreamModel', event.target.value)} placeholder={t('common.all')} /></label>
          <label>{t('usage.field.source_id')}<input value={draft.sourceId ?? ''} onChange={(event) => update('sourceId', event.target.value)} placeholder={t('common.all')} /></label>
          <label>{t('usage.field.account')}<input value={draft.account ?? ''} onChange={(event) => update('account', event.target.value)} placeholder={t('common.all')} /></label>
          <label>{t('usage.field.client_source')}<input value={draft.clientSource ?? ''} onChange={(event) => update('clientSource', event.target.value)} placeholder={t('common.all')} /></label>
          <label>{t('usage.field.protocol_in')}<input value={draft.protocolIn ?? ''} onChange={(event) => update('protocolIn', event.target.value)} placeholder={t('common.all')} /></label>
          <label>{t('usage.field.protocol_upstream')}<input value={draft.protocolUpstream ?? ''} onChange={(event) => update('protocolUpstream', event.target.value)} placeholder={t('common.all')} /></label>
          <label>{t('usage.field.virtual_key_id')}<input inputMode="numeric" value={draft.virtualKey ?? ''} onChange={(event) => update('virtualKey', event.target.value)} placeholder={t('common.all')} /></label>
          <label>{t('usage.field.usage_source')}<select value={draft.usageSource ?? ''} onChange={(event) => update('usageSource', event.target.value)}><option value="">{t('common.all')}</option><option value="upstream">{t('usage.usage_source.upstream')}</option><option value="parsed">{t('usage.usage_source.parsed')}</option><option value="estimated">{t('usage.usage_source.estimated')}</option><option value="missing">{t('usage.usage_source.missing')}</option></select></label>
        </div>
      )}
    </section>
  );
}
