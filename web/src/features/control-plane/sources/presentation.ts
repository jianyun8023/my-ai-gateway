import type {
  Account,
  SourceProtocolMode
} from '@/admin-api';

export const protocolModeTone = (mode: SourceProtocolMode | undefined) => {
  if (mode === 'native') return 'success' as const;
  if (mode === 'adapter') return 'warning' as const;
  if (mode === 'unsupported') return 'muted' as const;
  return 'accent' as const;
};

export const protocolModeKey = (mode: SourceProtocolMode | undefined) => (
  `sources.mode.${mode === 'native' || mode === 'adapter' || mode === 'unsupported' ? mode : 'unknown'}`
);

export const credentialKey = (account: Account) => (
  account.credential_configured ? 'sources.credential.configured' : 'sources.credential.not_configured'
);

/** 来源连接状态完全由账号真实健康数据推导，不虚构延迟或探测结果。 */
export type SourceConnectionState = 'healthy' | 'partial' | 'unknown' | 'none';

export interface SourceConnectionSummary {
  state: SourceConnectionState;
  healthy: number;
  total: number;
}

export const sourceConnection = (accounts: Account[]): SourceConnectionSummary => {
  const enabled = accounts.filter((account) => account.enabled);
  if (enabled.length === 0) return { state: 'none', healthy: 0, total: accounts.length };
  const healthy = enabled.filter((account) => account.health_status === 'healthy').length;
  if (healthy === enabled.length) return { state: 'healthy', healthy, total: enabled.length };
  if (healthy > 0) return { state: 'partial', healthy, total: enabled.length };
  return { state: enabled.every((account) => account.health_status === 'unknown') ? 'unknown' : 'partial', healthy, total: enabled.length };
};

export const sourceConnectionTone = (state: SourceConnectionState) => {
  if (state === 'healthy') return 'success' as const;
  if (state === 'partial') return 'warning' as const;
  return 'muted' as const;
};
