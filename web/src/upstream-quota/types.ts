export type UpstreamQuotaStatus =
  | 'ok'
  | 'low'
  | 'exhausted'
  | 'refreshing'
  | 'refresh_failed'
  | 'unsupported'
  | 'auth_error'
  | 'disabled';

export interface QuotaAccountView {
  account_id: string;
  account_display_name: string;
  source_id: string;
  source_display_name: string;
  provider_id: string;
  enabled: boolean;
}

export interface QuotaResource {
  type: 'window' | 'balance';
  key: string;
  label: string;
  unit: string;
  used?: number | null;
  remaining?: number | null;
  limit?: number | null;
  reset_at?: string | null;
}

export interface QuotaRefreshError {
  code: string;
  message: string;
  http_status?: number | null;
}

export interface UpstreamQuotaSnapshot {
  account: QuotaAccountView;
  status: UpstreamQuotaStatus;
  resources: QuotaResource[];
  fetched_at?: string | null;
  attempted_at: string;
  latency_ms: number;
  stale: boolean;
  refresh_error?: QuotaRefreshError | null;
  raw?: unknown;
}
