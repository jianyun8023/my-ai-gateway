import { clearOperationNotification } from '@/components/ui/notifications';
import type {
  AdminErrorShape,
  CatalogStatus,
  GatewayAdminResources,
  GatewayProtocol,
  SourceModel,
  SourceModelCapability,
  SourceModelCapabilityWrite,
  SourceProtocolMode
} from '@/admin-api';
import { GATEWAY_PROTOCOLS, normalizeAdminError } from '@/admin-api';
import { Button } from '@/components/ui/Button';
import { SelectField } from '@/components/ui/FormField';
import { LoadingState } from '@/components/ui/LoadingState';
import { StatusPill } from '@/components/ui/StatusPill';
import {
  IconCircleCheck
} from '@/components/ui/icons';
import styles from '@/features/control-plane/ControlPlane.module.scss';
import { statusTone } from '@/features/control-plane/discovery/model';
import { ErrorState, FormError, FormGrid, ProtocolPill } from '@/features/control-plane/shared';
import { useLocalizedApiError } from '@/hooks/useLocalizedApiError';
import { PROTOCOL_LABELS } from '@/lib/protocols';
import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';

const ADAPTER_OPTIONS: string[] = [];

const CAPABILITY_MODES: SourceProtocolMode[] = ['unknown', 'native', 'adapter', 'unsupported'];

const CAPABILITY_STATUSES: CatalogStatus[] = ['pending', 'confirmed', 'unavailable'];

interface CapabilityDraft {
  status: CatalogStatus;
  mode: SourceProtocolMode;
  source_protocol: GatewayProtocol | '';
  adapter: string;
}

const defaultCapabilityDraft = (record?: SourceModelCapability): CapabilityDraft => ({
  status: record?.status ?? 'pending',
  mode: record?.mode ?? 'unknown',
  source_protocol: record?.source_protocol ?? '',
  adapter: record?.adapter ?? '',
});

export function SourceModelCapabilitiesEditor({
  api,
  model,
  onSaved,
}: {
  api: GatewayAdminResources;
  model: SourceModel;
  onSaved: (protocol: GatewayProtocol) => void;
}) {
  const { t } = useTranslation('console');
  const localize = useLocalizedApiError();
  const [records, setRecords] = useState<SourceModelCapability[]>();
  const [loadError, setLoadError] = useState<AdminErrorShape>();
  const [drafts, setDrafts] = useState<Partial<Record<GatewayProtocol, CapabilityDraft>>>({});
  const [rowBusy, setRowBusy] = useState<GatewayProtocol>();
  const [rowErrors, setRowErrors] = useState<Partial<Record<GatewayProtocol, string>>>({});

  useEffect(() => {
    const controller = new AbortController();
    setRecords(undefined);
    setLoadError(undefined);
    api.sourceModelCapabilities(model.source_id, model.upstream_model_id, controller.signal)
      .then((items) => {
        if (controller.signal.aborted) return;
        setRecords(items);
        setDrafts(Object.fromEntries(
          GATEWAY_PROTOCOLS.map((protocol) => [
            protocol,
            defaultCapabilityDraft(items.find((item) => item.protocol === protocol)),
          ]),
        ) as Partial<Record<GatewayProtocol, CapabilityDraft>>);
      })
      .catch((error) => {
        if (!controller.signal.aborted) setLoadError(normalizeAdminError(error));
      });
    return () => controller.abort();
  }, [api, model.source_id, model.upstream_model_id]);

  const updateDraft = (protocol: GatewayProtocol, patch: Partial<CapabilityDraft>) => {
    setDrafts((current) => ({
      ...current,
      [protocol]: { ...defaultCapabilityDraft(), ...current[protocol], ...patch },
    }));
  };

  const save = async (protocol: GatewayProtocol) => {
    const draft = drafts[protocol];
    if (!draft || rowBusy) return;
    clearOperationNotification();
    setRowBusy(protocol);
    setRowErrors((current) => ({ ...current, [protocol]: undefined }));
    try {
      const input: SourceModelCapabilityWrite = {
        status: draft.status,
        mode: draft.mode,
        ...(draft.mode === 'adapter'
          ? {
              source_protocol: draft.source_protocol || undefined,
              adapter: draft.adapter || undefined,
            }
          : {}),
      };
      const result = await api.upsertSourceModelCapability(
        model.source_id,
        model.upstream_model_id,
        protocol,
        input,
      );
      setRecords((current) => [
        ...(current ?? []).filter((item) => item.protocol !== protocol),
        result.data,
      ]);
      onSaved(protocol);
    } catch (error) {
      setRowErrors((current) => ({ ...current, [protocol]: localize(error) }));
    } finally {
      setRowBusy(undefined);
    }
  };

  if (!records && !loadError) return <LoadingState label={t('discovery.capability_loading')} />;
  if (loadError) return <ErrorState error={loadError} />;

  return (
    <div className={styles.page}>
      {GATEWAY_PROTOCOLS.map((protocol) => {
        const record = records?.find((item) => item.protocol === protocol);
        const draft = drafts[protocol] ?? defaultCapabilityDraft(record);
        const adapterIncomplete = draft.mode === 'adapter' && (!draft.source_protocol || !draft.adapter);
        const unknownConfirm = draft.mode === 'unknown' && draft.status === 'confirmed';
        const saveDisabled = Boolean(rowBusy) || adapterIncomplete || unknownConfirm;
        return (
          <section key={protocol} className={styles.capabilityRow}>
            <header className={styles.capabilityRowHeader}>
              <ProtocolPill protocol={protocol} />
              {record ? (
                <span className={styles.rowActions}>
                  <StatusPill tone={statusTone(record.status)}>{t(`values.status.${record.status}`)}</StatusPill>
                  <StatusPill tone={record.mode === 'unsupported' ? 'warning' : 'accent'}>{t(`values.mode.${record.mode}`)}</StatusPill>
                </span>
              ) : (
                <StatusPill>{t('discovery.capability_undeclared')}</StatusPill>
              )}
            </header>
            <FormGrid>
              <SelectField
                label={t('discovery.capability_mode')}
                value={draft.mode}
                disabled={Boolean(rowBusy)}
                data={CAPABILITY_MODES.map((mode) => ({ value: mode, label: t(`values.mode.${mode}`) }))}
                onChange={(value) => updateDraft(protocol, { mode: value as SourceProtocolMode })}
              />
              <SelectField
                label={t('discovery.capability_status')}
                value={draft.status}
                disabled={Boolean(rowBusy)}
                data={CAPABILITY_STATUSES.map((status) => ({ value: status, label: t(`values.status.${status}`) }))}
                onChange={(value) => updateDraft(protocol, { status: value as CatalogStatus })}
              />
              {draft.mode === 'adapter' && (
                <>
                  <SelectField
                    label={t('discovery.capability_source_protocol')}
                    value={draft.source_protocol}
                    disabled={Boolean(rowBusy)}
                    data={[
                      { value: '', label: t('discovery.none') },
                      ...GATEWAY_PROTOCOLS.filter((item) => item !== protocol)
                        .map((item) => ({ value: item, label: PROTOCOL_LABELS[item] })),
                    ]}
                    onChange={(value) => updateDraft(protocol, { source_protocol: value as GatewayProtocol })}
                  />
                  <SelectField
                    label={t('discovery.capability_adapter')}
                    value={draft.adapter}
                    disabled={Boolean(rowBusy)}
                    data={[{ value: '', label: t('discovery.none') }, ...ADAPTER_OPTIONS.map((name) => ({ value: name, label: name }))]}
                    onChange={(value) => updateDraft(protocol, { adapter: value })}
                  />
                </>
              )}
            </FormGrid>
            {unknownConfirm && <small className={styles.secondaryText}>{t('discovery.capability_unknown_confirm_hint')}</small>}
            {draft.mode === 'adapter' && <small className={styles.secondaryText}>{t('discovery.capability_adapter_hint')}</small>}
            <FormError message={rowErrors[protocol]} />
            <div className={styles.rowActions}>
              <Button size="sm" variant="secondary" onClick={() => void save(protocol)} loading={rowBusy === protocol} disabled={saveDisabled}>
                <IconCircleCheck size={14} />{t('discovery.capability_save')}
              </Button>
            </div>
          </section>
        );
      })}
    </div>
  );
}
