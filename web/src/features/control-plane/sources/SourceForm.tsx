import type {
  GatewayProtocol,
  ProviderPreset,
  Source,
  SourceCreateInput,
  SourceProtocolMode,
  SourceWriteInput
} from '@/admin-api';
import { GATEWAY_PROTOCOLS } from '@/admin-api';
import { SelectField, TextAreaField, TextField } from '@/components/ui/FormField';
import styles from '@/features/control-plane/ControlPlane.module.scss';
import { CheckboxField, DrawerSection, FormError, FormGrid } from '@/features/control-plane/shared';
import { PROTOCOL_LABELS } from '@/lib/protocols';
import { useEffect, useMemo, useState, type FormEvent } from 'react';
import { useTranslation } from 'react-i18next';

type SourceField = 'id' | 'displayName' | 'preset' | 'baseUrl' | 'authHeader' | 'defaultHeaders' | `mode-${GatewayProtocol}`;

const presetEndpoints = (preset?: ProviderPreset): Partial<Record<GatewayProtocol, string>> => {
  const endpoints: Partial<Record<GatewayProtocol, string>> = {};
  for (const protocol of GATEWAY_PROTOCOLS) {
    const endpoint = preset?.definition.protocols?.[protocol]?.endpoint;
    if (endpoint) endpoints[protocol] = endpoint;
  }
  return endpoints;
};

const presetCapabilities = (preset?: ProviderPreset): Source['protocol_capabilities'] => {
  const capabilities: Source['protocol_capabilities'] = {};
  for (const protocol of GATEWAY_PROTOCOLS) {
    const definition = preset?.definition.protocols?.[protocol];
    if (definition) {
      capabilities[protocol] = {
        mode: definition.mode,
        source_protocol: definition.source_protocol,
        adapter: definition.adapter,
        features: definition.default_capabilities,
      };
    }
  }
  return capabilities;
};

const presetAuthConfig = (preset?: ProviderPreset): Source['auth_config'] => {
  const header = preset?.definition.credential_header;
  return header
    ? { credential_header: header, default_headers: preset?.definition.default_headers ?? {} }
    : {};
};

const authField = (authConfig: Source['auth_config'], field: 'header' | 'prefix'): string => {
  const credentialHeader = authConfig.credential_header;
  if (!credentialHeader || typeof credentialHeader !== 'object') return '';
  const value = (credentialHeader as Record<string, unknown>)[field];
  return typeof value === 'string' ? value : '';
};

const defaultHeadersJson = (authConfig: Source['auth_config']): string => {
  const value = authConfig.default_headers;
  return JSON.stringify(value && typeof value === 'object' ? value : {}, null, 2);
};

export function SourceForm({
  record,
  presets,
  busy,
  error,
  onSubmit,
  onDraftChange,
}: {
  record?: Source;
  presets: ProviderPreset[];
  busy: boolean;
  error?: string;
  onSubmit: (input: SourceCreateInput | SourceWriteInput) => void;
  /** 表单草稿（启用状态与三协议能力）变化时上报，供页面级摘要等展示同一份草稿。 */
  onDraftChange?: (draft: { enabled: boolean; capabilities: Source['protocol_capabilities'] }) => void;
}) {
  const { t } = useTranslation('console');
  const orderedPresets = useMemo(
    () => [...presets].sort((left, right) => left.id.localeCompare(right.id) || right.version - left.version),
    [presets],
  );
  const initialPreset = record
    ? orderedPresets.find((preset) => preset.id === record.provider_preset_id && preset.version === record.provider_preset_version)
    : orderedPresets.find((preset) => preset.id !== 'custom') ?? orderedPresets[0];
  const [id, setId] = useState(record?.id ?? '');
  const [displayName, setDisplayName] = useState(record?.display_name ?? '');
  const [presetKey, setPresetKey] = useState(
    initialPreset ? `${initialPreset.id}@${initialPreset.version}` : '',
  );
  const [baseUrl, setBaseUrl] = useState(record?.base_url ?? initialPreset?.definition.default_base_url ?? '');
  const [endpoints, setEndpoints] = useState<Partial<Record<GatewayProtocol, string>>>(() => (
    record?.endpoints
    ?? presetEndpoints(initialPreset)
  ));
  const [capabilities, setCapabilities] = useState<Source['protocol_capabilities']>(() => (
    record?.protocol_capabilities
    ?? presetCapabilities(initialPreset)
  ));
  const initialAuthConfig = record?.auth_config ?? presetAuthConfig(initialPreset);
  const [authHeader, setAuthHeader] = useState(() => authField(initialAuthConfig, 'header'));
  const [authPrefix, setAuthPrefix] = useState(() => authField(initialAuthConfig, 'prefix'));
  const [defaultHeaders, setDefaultHeaders] = useState(() => defaultHeadersJson(initialAuthConfig));
  const [enabled, setEnabled] = useState(record?.enabled ?? true);
  const [fieldErrors, setFieldErrors] = useState<Partial<Record<SourceField, string>>>({});

  // 让页面级摘要等展示与表单一致的草稿。
  useEffect(() => {
    onDraftChange?.({ enabled, capabilities });
  }, [enabled, capabilities, onDraftChange]);

  const selectedPreset = orderedPresets.find((preset) => `${preset.id}@${preset.version}` === presetKey);

  const clearFieldError = (field: SourceField) => {
    setFieldErrors((current) => {
      if (!current[field]) return current;
      const next = { ...current };
      delete next[field];
      return next;
    });
  };

  const rejectSubmission = (errors: Partial<Record<SourceField, string>>, focusOrder: SourceField[]) => {
    setFieldErrors(errors);
    const first = focusOrder.find((field) => errors[field]);
    if (first) document.getElementById(`source-${first}`)?.focus();
  };

  const selectPreset = (nextKey: string) => {
    clearFieldError('preset');
    setPresetKey(nextKey);
    const preset = orderedPresets.find((candidate) => `${candidate.id}@${candidate.version}` === nextKey);
    if (!preset || record) return;
    setBaseUrl(preset.definition.default_base_url ?? '');
    setEndpoints(presetEndpoints(preset));
    setCapabilities(presetCapabilities(preset));
    const authConfig = presetAuthConfig(preset);
    setAuthHeader(authField(authConfig, 'header'));
    setAuthPrefix(authField(authConfig, 'prefix'));
    setDefaultHeaders(defaultHeadersJson(authConfig));
  };

  const changeCapabilityMode = (protocol: GatewayProtocol, mode: SourceProtocolMode) => {
    setCapabilities((current) => {
      const previous = current[protocol];
      if (mode !== 'adapter') {
        return { ...current, [protocol]: { mode, features: previous?.features } };
      }
      return {
        ...current,
        [protocol]: {
          mode,
          source_protocol: previous?.source_protocol ?? GATEWAY_PROTOCOLS.find((candidate) => candidate !== protocol),
          adapter: previous?.adapter ?? '',
          features: previous?.features,
        },
      };
    });
  };

  const submit = (event: FormEvent) => {
    event.preventDefault();
    const errors: Partial<Record<SourceField, string>> = {};
    const requiredMessage = t('sources.form.validate_required');
    if (!id.trim()) errors.id = requiredMessage;
    if (!displayName.trim()) errors.displayName = requiredMessage;
    if (!presetKey) errors.preset = requiredMessage;
    if (!baseUrl.trim()) errors.baseUrl = requiredMessage;
    if (!selectedPreset && !record) {
      errors.preset = t('sources.form.validate_preset');
    }
    GATEWAY_PROTOCOLS.forEach((protocol) => {
      const capability = capabilities[protocol];
      if (capability?.mode === 'adapter' && (!capability.source_protocol || !capability.adapter?.trim())) {
        errors[`mode-${protocol}`] = t('sources.form.validate_adapter_mode');
      }
    });
    let parsedHeaders: unknown;
    try {
      parsedHeaders = JSON.parse(defaultHeaders || '{}');
    } catch {
      errors.defaultHeaders = t('sources.form.validate_headers_json');
    }
    if (!errors.defaultHeaders && (!parsedHeaders || typeof parsedHeaders !== 'object' || Array.isArray(parsedHeaders))) {
      errors.defaultHeaders = t('sources.form.validate_headers_object');
    }
    if (!authHeader.trim()) {
      errors.authHeader = t('sources.form.validate_headers_nonempty');
    }
    if (!errors.defaultHeaders && !Object.values(parsedHeaders as Record<string, unknown>).every((value) => typeof value === 'string')) {
      errors.defaultHeaders = t('sources.form.validate_headers_string');
    }
    const focusOrder: SourceField[] = [
      'id', 'displayName', 'preset', 'baseUrl',
      ...GATEWAY_PROTOCOLS.map((protocol) => `mode-${protocol}` as const),
      'authHeader', 'defaultHeaders',
    ];
    if (Object.keys(errors).length > 0) {
      rejectSubmission(errors, focusOrder);
      return;
    }
    const authConfig = {
      credential_header: { header: authHeader.trim(), prefix: authPrefix },
      default_headers: parsedHeaders as Record<string, string>,
    };
    setFieldErrors({});
    if (record) {
      onSubmit({
        id: record.id,
        display_name: displayName.trim(),
        provider_preset_id: record.provider_preset_id,
        provider_preset_version: record.provider_preset_version,
        base_url: baseUrl.trim(),
        endpoints,
        auth_config: authConfig,
        protocol_capabilities: capabilities,
        enabled,
      });
      return;
    }
    onSubmit({
      id: id.trim(),
      display_name: displayName.trim(),
      provider_preset_id: selectedPreset!.id,
      provider_preset_version: selectedPreset!.version,
      base_url: baseUrl.trim(),
      endpoint_overrides: endpoints,
      protocol_capabilities: capabilities,
      auth_config: authConfig,
      enabled,
    });
  };

  return (
    <form id="source-editor-form" className={styles.page} onSubmit={submit}>
      <FormError message={error} />
      <FormGrid>
        <TextField id="source-id" label={t('sources.field.source_id')} value={id} error={fieldErrors.id} disabled={Boolean(record) || busy} onChange={(event) => { clearFieldError('id'); setId(event.target.value); }} autoComplete="off" />
        <TextField id="source-displayName" label={t('sources.field.display_name')} value={displayName} error={fieldErrors.displayName} disabled={busy} onChange={(event) => { clearFieldError('displayName'); setDisplayName(event.target.value); }} autoComplete="off" />
        <SelectField
          id="source-preset"
          label={t('sources.field.provider_preset')}
          value={presetKey}
          error={fieldErrors.preset}
          disabled={Boolean(record) || busy || orderedPresets.length === 0}
          data={orderedPresets.length === 0
            ? [{ value: '', label: t('sources.form.no_preset') }]
            : orderedPresets.map((preset) => ({
                value: `${preset.id}@${preset.version}`,
                label: `${preset.display_name} · ${preset.id}@${preset.version}`,
              }))}
          onChange={selectPreset}
        />
        <TextField id="source-baseUrl" label={t('sources.field.base_url')} type="url" value={baseUrl} error={fieldErrors.baseUrl} disabled={busy} onChange={(event) => { clearFieldError('baseUrl'); setBaseUrl(event.target.value); }} autoComplete="off" />
        {(['openai_chat_completions', 'openai_responses', 'anthropic_messages'] as const).map((protocol) => (
          <TextField
            key={protocol}
            label={t('sources.form.endpoint_hint', { protocol: PROTOCOL_LABELS[protocol] })}
            value={endpoints[protocol] ?? ''}
            disabled={busy}
            onChange={(event) => setEndpoints((current) => ({ ...current, [protocol]: event.target.value }))}
            autoComplete="off"
          />
        ))}
        <div className={styles.fullWidth}>
          <CheckboxField checked={enabled} disabled={busy} onChange={setEnabled} label={t('sources.field.enable_source')} />
        </div>
      </FormGrid>
      <DrawerSection title={t('sources.form.section_capabilities')} hint={t('sources.form.adapter_unavailable')}>
        <div className={styles.protocolCapabilityEditor}>
          {GATEWAY_PROTOCOLS.map((protocol) => {
            const capability = capabilities[protocol];
            const mode = capability?.mode ?? 'unknown';
            return (
              <div key={protocol}>
                <strong>{PROTOCOL_LABELS[protocol]}</strong>
                <SelectField
                  id={`source-mode-${protocol}`}
                  label={t('sources.form.mode')}
                  value={mode}
                  error={fieldErrors[`mode-${protocol}`]}
                  disabled={busy}
                  data={[
                    { value: 'native', label: t('sources.mode.native') },
                    { value: 'adapter', label: t('sources.mode.adapter'), disabled: mode !== 'adapter' },
                    { value: 'unsupported', label: t('sources.mode.unsupported') },
                    { value: 'unknown', label: t('sources.mode.unknown'), disabled: true },
                  ]}
                  onChange={(value) => { clearFieldError(`mode-${protocol}`); changeCapabilityMode(protocol, value as SourceProtocolMode); }}
                />
                {mode === 'adapter' && (
                  <>
                    <SelectField
                      label={t('sources.form.upstream_protocol')}
                      value={capability?.source_protocol ?? ''}
                      disabled
                      data={GATEWAY_PROTOCOLS.filter((candidate) => candidate !== protocol)
                        .map((candidate) => ({ value: candidate, label: PROTOCOL_LABELS[candidate] }))}
                      onChange={(value) => setCapabilities((current) => ({
                        ...current,
                        [protocol]: {
                          ...current[protocol],
                          mode: 'adapter',
                          source_protocol: value as GatewayProtocol,
                          adapter: current[protocol]?.adapter ?? '',
                        },
                      }))}
                    />
                    <TextField label={t('sources.form.adapter')} value={capability?.adapter ?? ''} disabled onChange={(event) => setCapabilities((current) => ({
                      ...current,
                      [protocol]: {
                        ...current[protocol],
                        mode: 'adapter',
                        source_protocol: current[protocol]?.source_protocol ?? GATEWAY_PROTOCOLS.find((candidate) => candidate !== protocol),
                        adapter: event.target.value,
                      },
                    }))} autoComplete="off" spellCheck={false} />
                  </>
                )}
              </div>
            );
          })}
        </div>
      </DrawerSection>
      <DrawerSection title={t('sources.form.section_auth')}>
        <FormGrid>
          <TextField id="source-authHeader" label={t('sources.form.credential_headers')} value={authHeader} error={fieldErrors.authHeader} disabled={busy} onChange={(event) => { clearFieldError('authHeader'); setAuthHeader(event.target.value); }} autoComplete="off" spellCheck={false} />
          <TextField label={t('sources.form.header_prefix')} value={authPrefix} disabled={busy} onChange={(event) => setAuthPrefix(event.target.value)} autoComplete="off" spellCheck={false} />
          <TextAreaField id="source-defaultHeaders" label={t('sources.form.default_headers')} value={defaultHeaders} error={fieldErrors.defaultHeaders} disabled={busy} onChange={(event) => { clearFieldError('defaultHeaders'); setDefaultHeaders(event.target.value); }} spellCheck={false} />
        </FormGrid>
      </DrawerSection>
    </form>
  );
}
