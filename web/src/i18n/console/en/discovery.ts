// Model update review copy (English) — console.discovery namespace
export const discovery = {
  filters_aria: 'Model review filters',
  // Metadata field sources (values keep enum form)
  field_source: {
    user: 'User',
    preset: 'Preset',
    upstream: 'Upstream',
    unknown: 'Unknown',
  },
  confirm_state: {
    pending: 'Pending review',
    confirmed: 'Confirmed',
    unavailable: 'Marked unavailable',
  },
  availability_state: {
    unknown: 'Unknown',
    available: 'Available',
    unavailable: 'Unavailable',
  },
  // Discovery run states
  run_state: {
    succeeded: 'Succeeded',
    failed: 'Failed',
    unsupported: 'Unsupported',
  },
  // Discovery change kinds
  change_kind: {
    added: 'New model',
    changed: 'Field changes',
    changed_with_fields: 'Field changes: {{fields}}',
    missing: 'Missing',
    unknown: 'No comparison available',
  },
  // Capability summary features
  feature: {
    tools: 'Tools',
    thinking: 'Thinking',
    web_search: 'Web Search',
    structured_output: 'Structured output',
  },

  // Common
  diff_none: 'No changes',
  unnamed_metadata: 'Unnamed metadata',
  validate_tokens: 'Token numeric fields must be empty or greater than 0.',
  no_field_changes: 'No field changes to save.',
  none: 'none',

  // Run/result states
  loading_run: 'Loading the latest model check…',
  state_unsupported: 'This provider preset does not support syncing the upstream model catalog, so model update checks are unavailable.',
  state_failed: 'Model update check failed',
  state_failed_desc: 'Check the source connection and account credentials, then retry.',
  state_empty: 'The check succeeded but the upstream returned no models',
  state_empty_desc: 'Existing models are unchanged.',

  // Messages
  message_saved: 'User fields for SourceModel {{name}} saved.',
  message_confirmed: '{{count}} SourceModels confirmed.',

  // Main page
  loading: 'Loading model update review…',
  confirmation_filter: 'Confirmation status',
  availability_filter: 'Availability status',
  source_model_count: '{{count}} SourceModels',
  loading_models: 'Loading SourceModels…',
  empty_no_auto: 'This source does not support automatic model checks',
  empty_no_match: 'No SourceModels match the current filters',
  empty_no_match_desc: 'The check failed. Check the connection and retry.',
  empty_no_auto_desc: 'Run a model check or adjust the filters.',
  models_card: 'Upstream Models',
  confirm_selected: 'Confirm Selected',
  edit_pending_title: 'Edit Pending SourceModel',
  save_user_fields: 'Save User Fields',
  batch_confirm_title: 'Confirm SourceModels',
  batch_confirm_desc: 'Confirm {{count}} selected models? Confirmed models become selectable in Models & Routes, but bindings and routes must still be created separately.',
  confirm_models: 'Confirm Models',

  // Protocol capability editing
  capabilities_action: 'Capabilities',
  capabilities_title: 'Protocol capabilities — {{model}}',
  capabilities_subtitle: 'Declare how each ingress protocol is handled; bindings require a confirmed capability.',
  capability_loading: 'Loading protocol capabilities…',
  capability_undeclared: 'Undeclared',
  capability_mode: 'Mode',
  capability_status: 'Status',
  capability_source_protocol: 'Upstream protocol',
  capability_adapter: 'Adapter',
  capability_save: 'Save capability',
  capability_saved: '{{protocol}} capability saved.',
  capability_unknown_confirm_hint: 'Unknown capabilities cannot be confirmed.',
  capability_adapter_hint: 'Adapter mode requires a confirmed native upstream protocol.',
  capability_adapter_unavailable: 'No production adapter is available. Existing adapter declarations are read-only and cannot be created or changed.',

  // Table
  column: {
    upstream_model: 'Upstream model',
    confirmation: 'Confirmation',
    availability: 'Availability',
  },
  diff_column: {
    added: 'Added',
    changed: 'Changed',
    missing: 'Missing',
  },
  select_all_aria: 'Select all confirmable models',
  select_row_aria: 'Select {{model}}',
  table_aria: 'Upstream model review table',
} as const;
