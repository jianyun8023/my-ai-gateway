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
