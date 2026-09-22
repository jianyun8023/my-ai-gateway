export const GATEWAY_PROTOCOLS = [
  'openai_chat_completions',
  'openai_responses',
  'anthropic_messages',
] as const;

export type GatewayProtocol = typeof GATEWAY_PROTOCOLS[number];

export interface FilterOptionsResponse {
  data: string[];
  has_more: boolean;
}

export interface FilterOptionsRange {
  from?: string;
  to?: string;
}
export type CatalogStatus = 'pending' | 'confirmed' | 'unavailable';
export type CatalogAvailability = 'unknown' | 'available' | 'unavailable';
export type MetadataSource = 'upstream' | 'preset' | 'user' | 'unknown';
type CapabilitySupport = 'supported' | 'unsupported' | 'unknown';
export type SourceProtocolMode = 'unknown' | 'native' | 'adapter' | 'unsupported';
type RuntimeProtocolMode = 'native' | 'adapter';
type EffectiveCapabilityMode = 'native' | 'translated' | 'unsupported';

type JsonObject = Record<string, unknown>;

export interface AdminErrorShape {
  status?: number;
  code?: string;
  message: string;
}

export interface AdminDataEnvelope<T> {
  data: T;
}

export interface AdminMutationEnvelope<T> extends AdminDataEnvelope<T> {
  snapshot_revision: number;
  snapshot_generated_at: string;
}

interface CredentialHeaderTemplate {
  header: string;
  prefix: string;
}

interface ConnectionTestTemplate {
  method: 'get' | 'post';
  default_model: string;
  body: JsonObject;
}

interface ProviderProtocolPreset {
  endpoint: string;
  mode: SourceProtocolMode;
  source_protocol?: GatewayProtocol;
  adapter?: string;
  headers: Record<string, string>;
  default_capabilities: Record<string, CapabilitySupport>;
  connection_test: ConnectionTestTemplate;
}

type ProviderDiscoveryDefinition =
  | {
      support: 'supported';
      method: 'get' | 'post';
      endpoint: string;
      parser: {
        list_path: string;
        id_path: string;
        metadata_paths: Record<string, string>;
      };
    }
  | {
      support: 'unsupported';
      reason: string;
    };

interface ProviderPresetDefinition {
  schema_version?: number;
  default_base_url?: string;
  credential_header?: CredentialHeaderTemplate;
  default_headers?: Record<string, string>;
  protocols?: Partial<Record<GatewayProtocol, ProviderProtocolPreset>>;
  discovery?: ProviderDiscoveryDefinition;
}

export interface ProviderPreset {
  id: string;
  version: number;
  display_name: string;
  definition: ProviderPresetDefinition;
  created_at: string;
}

interface SourceProtocolCapability {
  mode: SourceProtocolMode;
  source_protocol?: GatewayProtocol;
  adapter?: string;
  features?: Record<string, CapabilitySupport>;
}

export interface Source {
  id: string;
  display_name: string;
  provider_preset_id: string;
  provider_preset_version: number;
  provider_preset_snapshot: ProviderPresetDefinition | JsonObject;
  base_url: string;
  endpoints: Partial<Record<GatewayProtocol, string>>;
  auth_config: JsonObject;
  protocol_capabilities: Partial<Record<GatewayProtocol, SourceProtocolCapability>>;
  enabled: boolean;
  created_at: string;
  updated_at: string;
}

export interface SourceCreateInput {
  id: string;
  display_name: string;
  provider_preset_id: string;
  provider_preset_version?: number;
  base_url?: string;
  endpoints?: Partial<Record<GatewayProtocol, string>>;
  endpoint_overrides?: Partial<Record<GatewayProtocol, string>>;
  auth_config?: JsonObject;
  protocol_capabilities?: Partial<Record<GatewayProtocol, SourceProtocolCapability>>;
  enabled?: boolean;
}

export interface SourceWriteInput {
  id: string;
  display_name: string;
  provider_preset_id: string;
  provider_preset_version: number;
  base_url: string;
  endpoints: Partial<Record<GatewayProtocol, string>>;
  auth_config: JsonObject;
  protocol_capabilities: Partial<Record<GatewayProtocol, SourceProtocolCapability>>;
  enabled: boolean;
}

export interface Account {
  id: string;
  source_id: string;
  display_name: string;
  credential_env?: string | null;
  credential_configured: boolean;
  enabled: boolean;
  weight: number;
  health_status: string;
  health_updated_at?: string | null;
  last_probe_at?: string | null;
  last_probe_status?: string | null;
  cooldown_until?: string | null;
  created_at: string;
  updated_at: string;
}

export interface AccountHealthView {
  account_id: string;
  source_id: string;
  display_name: string;
  enabled: boolean;
  health_status: string;
  health_updated_at?: string | null;
  stale: boolean;
  cooldown_until?: string | null;
  health: {
    status: string;
    stale: boolean;
    updated_at?: string | null;
    last_probe_at?: string | null;
    cooldown_until?: string | null;
    last_error?: string | null;
    last_success_at?: string | null;
  };
}

export interface AccountHealthResponse {
  fact_source: string;
  stale_after_secs: number;
  data: AccountHealthView[];
}

export interface AccountWriteInput {
  id: string;
  source_id: string;
  display_name: string;
  credential_env?: string | null;
  credential_ciphertext?: string | null;
  enabled: boolean;
  weight: number;
}

interface PresetDiffEntry {
  path: string;
  kind: 'added' | 'changed' | 'missing';
  before?: unknown;
  after?: unknown;
}

export interface ProviderPresetDiff {
  source_id: string;
  provider_preset_id: string;
  source_version: number;
  latest_version: number;
  changes: PresetDiffEntry[];
}

export interface ConnectionTestResult {
  id: number;
  source_id: string;
  account_id?: string | null;
  protocol: GatewayProtocol;
  upstream_protocol: GatewayProtocol;
  mode: SourceProtocolMode;
  status: string;
  http_status?: number | null;
  latency_ms: number;
  error_code?: string | null;
  error_message?: string | null;
  requested_by: string;
  tested_at: string;
}

export interface ConnectionTestInput {
  account_id: string;
  protocol: GatewayProtocol;
  model?: string;
  requested_by?: string;
}

export const MODEL_METADATA_FIELDS = [
  'logical_model_name',
  'display_name',
  'context_window',
  'max_input_tokens',
  'max_output_tokens',
  'input_modalities',
  'output_modalities',
  'tools',
  'thinking',
  'web_search',
  'structured_output',
  'streaming',
  'usage',
] as const;

export type ModelMetadataField = typeof MODEL_METADATA_FIELDS[number];

export type ModelMetadataValues = Partial<Record<ModelMetadataField, unknown>>;

export interface SourceModel {
  source_id: string;
  upstream_model_id: string;
  confirmation_status: CatalogStatus;
  availability_status: CatalogAvailability;
  raw_snapshot: unknown;
  metadata: ModelMetadataValues;
  field_sources: Partial<Record<ModelMetadataField, MetadataSource>>;
  matched_model_preset_id?: string | null;
  matched_model_preset_version?: number | null;
  first_discovered_at: string;
  last_discovered_at: string;
  confirmed_at?: string | null;
  unavailable_at?: string | null;
  created_at: string;
  updated_at: string;
}

interface DiscoveryDiffEntry {
  upstream_model_id: string;
  changed_fields: string[];
}

export interface DiscoveryDiff {
  added: DiscoveryDiffEntry[];
  changed: DiscoveryDiffEntry[];
  missing: DiscoveryDiffEntry[];
}

interface DiscoveryRun {
  id: number;
  source_id: string;
  account_id?: string | null;
  provider_preset_id: string;
  provider_preset_version: number;
  status: 'succeeded' | 'failed' | 'unsupported' | string;
  raw_snapshot?: unknown;
  diff: DiscoveryDiff;
  discovered_model_count: number;
  http_status?: number | null;
  latency_ms: number;
  error_code?: string | null;
  error_message?: string | null;
  requested_by: string;
  started_at: string;
  completed_at: string;
}

export interface DiscoveryExecution {
  run: DiscoveryRun;
  diff: DiscoveryDiff;
  models: SourceModel[];
}

export interface LatestDiscovery {
  run: DiscoveryRun;
  diff: DiscoveryDiff;
  last_discovered_at: string;
}

export interface SourceModelFilters {
  confirmationStatus?: CatalogStatus;
  availabilityStatus?: CatalogAvailability;
}

export interface SourceModelEditInput {
  upstream_model_id: string;
  metadata: ModelMetadataValues;
}

export interface SourceModelConfirmation {
  upstream_model_id: string;
  metadata?: ModelMetadataValues;
}

export interface SourceModelCapability {
  source_id: string;
  upstream_model_id: string;
  protocol: GatewayProtocol;
  status: CatalogStatus;
  mode: SourceProtocolMode;
  source_protocol?: GatewayProtocol | null;
  adapter?: string | null;
  feature_capabilities: Record<string, CapabilitySupport>;
  field_source: string;
  observed_at: string;
  confirmed_at?: string | null;
  unavailable_at?: string | null;
  updated_at: string;
}

export interface SourceModelCapabilityWrite {
  status: CatalogStatus;
  mode: SourceProtocolMode;
  source_protocol?: GatewayProtocol;
  adapter?: string;
  feature_capabilities?: Record<string, CapabilitySupport>;
}

export interface LogicalModel {
  id: string;
  public_name: string;
  display_name: string;
  status: CatalogStatus;
  metadata: ModelMetadataValues;
  field_sources: Partial<Record<ModelMetadataField, MetadataSource>>;
  enabled: boolean;
  request_timeout_ms: number | null;
  max_retries: number | null;
  confirmed_at?: string | null;
  unavailable_at?: string | null;
  created_at: string;
  updated_at: string;
}

export interface LogicalModelWriteInput {
  id: string;
  public_name: string;
  display_name: string;
  status: CatalogStatus;
  metadata: ModelMetadataValues;
  field_sources: Partial<Record<ModelMetadataField, MetadataSource>>;
  enabled: boolean;
}

export interface ModelRoutingLineInput {
  source_id: string;
  account_id: string;
  upstream_model_id: string;
}

export interface ModelRoutingWriteInput {
  public_name: string;
  display_name: string;
  enabled: boolean;
  lines: ModelRoutingLineInput[];
  request_timeout_ms: number | null;
  max_retries: number | null;
}

export interface ModelRoutingConfiguration {
  logical_model: LogicalModel;
  lines: Array<ModelRoutingLineInput & { protocols: GatewayProtocol[] }>;
  protocols: GatewayProtocol[];
  strategy: string;
  request_timeout_ms: number | null;
  max_retries: number | null;
}

export interface ModelBinding {
  id: number;
  logical_model_id: string;
  source_id: string;
  account_id: string;
  upstream_model_id: string;
  protocol: GatewayProtocol;
  status: CatalogStatus;
  enabled: boolean;
  priority: number;
  confirmed_at?: string | null;
  unavailable_at?: string | null;
  created_at: string;
  updated_at: string;
}

export interface ModelBindingWriteInput {
  logical_model_id: string;
  source_id: string;
  account_id: string;
  upstream_model_id: string;
  protocol: GatewayProtocol;
  status: CatalogStatus;
  enabled: boolean;
  priority: number;
}

export interface Route {
  id: string;
  logical_model_id: string;
  public_name: string;
  protocols: GatewayProtocol[];
  strategy: string;
  allow_lossy_conversion: boolean;
  enabled: boolean;
  created_at: string;
  updated_at: string;
}

export interface RouteWriteInput {
  id: string;
  logical_model_id: string;
  protocols: GatewayProtocol[];
  strategy: string;
  allow_lossy_conversion: boolean;
  enabled: boolean;
}

interface EffectiveCapabilities {
  streaming: EffectiveCapabilityMode;
  tools: EffectiveCapabilityMode;
  tool_streaming: EffectiveCapabilityMode;
  thinking: EffectiveCapabilityMode;
  web_search: EffectiveCapabilityMode;
  file_search: EffectiveCapabilityMode;
  vision: EffectiveCapabilityMode;
  usage: EffectiveCapabilityMode;
}

interface RouteResolutionError {
  code: string;
  message: string;
  route_id?: string | null;
}

interface ProtocolConversionHop {
  protocol_from: GatewayProtocol;
  protocol_to: GatewayProtocol;
  mode: RuntimeProtocolMode;
  adapter?: string | null;
}

export interface EffectiveProtocolCapability {
  protocol_in: GatewayProtocol;
  status: 'routable' | 'unroutable';
  binding_id?: number | null;
  selection?: 'primary' | 'fallback' | null;
  selection_rank?: number | null;
  protocol_upstream?: GatewayProtocol | null;
  endpoint?: string | null;
  mode?: RuntimeProtocolMode | null;
  adapter?: string | null;
  conversion_chain: ProtocolConversionHop[];
  effective_capabilities: EffectiveCapabilities;
  degraded: boolean;
  degraded_features: string[];
  allow_lossy_conversion?: boolean | null;
  error?: RouteResolutionError | null;
}

export interface CapabilityMatrixRow {
  route_id: string;
  source: {
    source_id: string;
    display_name?: string | null;
  };
  account: {
    account_id: string;
    display_name?: string | null;
    enabled?: boolean | null;
  };
  model: string;
  model_display_name: string;
  upstream_model_id: string;
  protocols: EffectiveProtocolCapability[];
}

export interface CapabilityMatrixResponse {
  version: 'v1' | string;
  fact_source: 'runtime_snapshot';
  snapshot_revision: number;
  snapshot_generated_at: string;
  data: CapabilityMatrixRow[];
}

export interface VirtualKey {
  id: number;
  name: string;
  key_prefix: string;
  key_recoverable: boolean;
  allowed_models: string[];
  enabled: boolean;
  created_at: string;
  last_used_at?: string | null;
  revoked_at?: string | null;
  expires_at?: string | null;
  replaced_by_id?: number | null;
  overlap_until?: string | null;
}

export interface VirtualKeySecret {
  id: number;
  key: string;
}

export interface VirtualKeyCreateInput {
  name: string;
  allowed_models: string[];
}

export interface VirtualKeyCreateResult extends VirtualKeyCreateInput {
  id: number;
  key: string;
}

export interface VirtualKeyRotateInput {
  overlap_secs: number;
  allowed_models: string[];
}

export interface VirtualKeyRotateResult {
  old_id: number;
  new_id: number;
  key_prefix: string;
  key: string;
  overlap_until: string | null;
}

export interface RuntimeReloadResult {
  status: 'reloaded';
  snapshot_revision: number;
  snapshot_generated_at: string;
}

export const RUNTIME_EVENT_CATEGORIES = [
  'lifecycle',
  'configuration',
  'database',
  'security',
  'request',
  'health',
  'operation',
  'admin',
  'discovery',
] as const;

export const RUNTIME_EVENT_LEVELS = ['info', 'warning', 'error'] as const;

export const RUNTIME_EVENT_SOURCES = [
  'system_events',
  'usage_events',
  'account_health_events',
  'audit_logs',
  'source_discovery_runs',
] as const;

export type RuntimeEventCategory = typeof RUNTIME_EVENT_CATEGORIES[number];
export type RuntimeEventLevel = typeof RUNTIME_EVENT_LEVELS[number];
export type RuntimeEventSource = typeof RUNTIME_EVENT_SOURCES[number];

export interface RuntimeEventRecord {
  event_id: string;
  occurred_at: string;
  category: RuntimeEventCategory;
  event_type: string;
  level: RuntimeEventLevel;
  subject_type: string;
  subject_id?: string | null;
  correlation_id?: string | null;
  message: string;
  details: JsonObject;
  source: RuntimeEventSource;
}

export interface RuntimeEventFilters {
  from?: string;
  since?: string;
  to?: string;
  category?: RuntimeEventCategory;
  level?: RuntimeEventLevel;
  event_type?: string;
  subject_type?: string;
  subject_id?: string;
  correlation_id?: string;
  source?: RuntimeEventSource;
  limit?: number;
  cursor?: string;
}

export interface RuntimeEventResponse {
  version: 'v1' | string;
  timezone: 'UTC' | string;
  fact_source: 'postgresql_unified_read_model' | string;
  range: {
    from?: string | null;
    since?: string | null;
    to?: string | null;
    boundary: string;
  };
  data: RuntimeEventRecord[];
  page: {
    limit: number;
    has_more: boolean;
    next_cursor?: string | null;
  };
}

export interface SanitizedConfigurationExport {
  version: 'v1';
  exported_at: string;
  fact_source: 'admin_resources';
  snapshot: {
    revision: number;
    generated_at: string;
  };
  sources: Source[];
  accounts: Array<Omit<Account, 'credential_env'>>;
  logical_models: LogicalModel[];
  model_bindings: ModelBinding[];
  routes: Route[];
}
