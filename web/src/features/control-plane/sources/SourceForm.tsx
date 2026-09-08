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
import { useMemo, useState, type FormEvent } from 'react';
import { useTranslation } from 'react-i18next';

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
}: {
  record?: Source;
  presets: ProviderPreset[];
  busy: boolean;
  error?: string;
  onSubmit: (input: SourceCreateInput | SourceWriteInput) => void;
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
  const [validationError, setValidationError] = useState('');

  const selectedPreset = orderedPresets.find((preset) => `${preset.id}@${preset.version}` === presetKey);

  const selectPreset = (nextKey: string) => {
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
    if (!id.trim() || !displayName.trim() || !presetKey || !baseUrl.trim()) {
      setValidationError(t('sources.form.validate_required_source'));
      return;
    }
    if (!selectedPreset && !record) {
      setValidationError(t('sources.form.validate_preset'));
      return;
    }
    const invalidAdapter = GATEWAY_PROTOCOLS.some((protocol) => {
      const capability = capabilities[protocol];
      return capability?.mode === 'adapter'
        && (!capability.source_protocol || !capability.adapter?.trim());
    });
    if (invalidAdapter) {
      setValidationError(t('sources.form.validate_adapter_mode'));
      return;
    }
    let parsedHeaders: unknown;
    try {
      parsedHeaders = JSON.parse(defaultHeaders || '{}');
    } catch {
      setValidationError(t('sources.form.validate_headers_json'));
      return;
    }
    if (!parsedHeaders || typeof parsedHeaders !== 'object' || Array.isArray(parsedHeaders)) {
      setValidationError(t('sources.form.validate_headers_object'));
      return;
    }
    if (!authHeader.trim()) {
      setValidationError(t('sources.form.validate_headers_nonempty'));
      return;
    }
    if (!Object.values(parsedHeaders).every((value) => typeof value === 'string')) {
      setValidationError(t('sources.form.validate_headers_string'));
      return;
    }
    const authConfig = {
      credential_header: { header: authHeader.trim(), prefix: authPrefix },
      default_headers: parsedHeaders as Record<string, string>,
    };
    setValidationError('');
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
      <FormGrid>
        <TextField label={t('sources.field.source_id')} value={id} disabled={Boolean(record) || busy} onChange={(event) => setId(event.target.value)} autoComplete="off" />
        <TextField label={t('sources.field.display_name')} value={displayName} disabled={busy} onChange={(event) => setDisplayName(event.target.value)} autoComplete="off" />
        <SelectField
          label={t('sources.field.provider_preset')}
          value={presetKey}
          disabled={Boolean(record) || busy || orderedPresets.length === 0}
          data={orderedPresets.length === 0
            ? [{ value: '', label: t('sources.form.no_preset') }]
            : orderedPresets.map((preset) => ({
                value: `${preset.id}@${preset.version}`,
                label: `${preset.display_name} · ${preset.id}@${preset.version}`,
              }))}
          onChange={selectPreset}
        />
        <TextField label={t('sources.field.base_url')} type="url" value={baseUrl} disabled={busy} onChange={(event) => setBaseUrl(event.target.value)} autoComplete="off" />
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
      <DrawerSection title={t('sources.form.section_capabilities')}>
        <div className={styles.protocolCapabilityEditor}>
          {GATEWAY_PROTOCOLS.map((protocol) => {
            const capability = capabilities[protocol];
            const mode = capability?.mode ?? 'unknown';
            return (
              <div key={protocol}>
                <strong>{PROTOCOL_LABELS[protocol]}</strong>
                <SelectField
                  label={t('sources.form.mode')}
                  value={mode}
                  disabled={busy}
                  data={[
                    { value: 'native', label: t('sources.mode.native') },
                    { value: 'adapter', label: t('sources.mode.adapter') },
                    { value: 'unsupported', label: t('sources.mode.unsupported') },
                    { value: 'unknown', label: t('sources.mode.unknown'), disabled: true },
                  ]}
                  onChange={(value) => changeCapabilityMode(protocol, value as SourceProtocolMode)}
                />
                {mode === 'adapter' && (
                  <>
                    <SelectField
                      label={t('sources.form.upstream_protocol')}
                      value={capability?.source_protocol ?? ''}
                      disabled={busy}
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
                    <TextField label={t('sources.form.adapter')} value={capability?.adapter ?? ''} disabled={busy} onChange={(event) => setCapabilities((current) => ({
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
          <TextField label={t('sources.form.credential_headers')} value={authHeader} disabled={busy} onChange={(event) => setAuthHeader(event.target.value)} autoComplete="off" spellCheck={false} />
          <TextField label={t('sources.form.header_prefix')} value={authPrefix} disabled={busy} onChange={(event) => setAuthPrefix(event.target.value)} autoComplete="off" spellCheck={false} />
          <TextAreaField label={t('sources.form.default_headers')} value={defaultHeaders} disabled={busy} onChange={(event) => setDefaultHeaders(event.target.value)} spellCheck={false} />
        </FormGrid>
      </DrawerSection>
      <FormError message={validationError || error} />
    </form>
  );
}
