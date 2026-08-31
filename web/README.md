# my-ai-gateway Usage Console

The active web application is a gateway-native, token-first usage console with
three pages:

- Overview: logical requests, upstream attempts, success rate, token
  composition, timeseries, and recent activity.
- Analysis: logical/upstream model, Provider, Source, Account, protocol, and
  latency breakdowns.
- Request Events: combination filters, deterministic cursor pagination,
  virtual scrolling, column preferences, metadata-only details, and CSV/JSON
  exports.

## Data boundary

Runtime requests are restricted to `/admin/usage/*`. The UI does not call CPA
Usage Keeper endpoints and does not mount its session, Ranking, Auth Files,
AI Provider credentials, quota, pricing, Management API, or request-log body
features.

The `src/gateway-usage` boundary isolates the HTTP wire contract from page view
models. Its client mirrors Issue #15's versioned `data`/`page` envelope,
combination filters, deterministic cursor, breakdown dimensions, and export
routes; fixtures keep those semantics testable while the backend PR is still
being finalized. Usage events distinguish logical requests from upstream
attempts, store UTC boundaries, and display timestamps in the browser's local
timezone.

An Admin key can be entered in the header when `GATEWAY_ADMIN_KEY` (or the
temporary `GATEWAY_API_KEY` fallback) protects the Admin API. It is kept only in
`sessionStorage`; no Admin Session login is implemented here.

## Development

```bash
npm ci
npm run typecheck
npm run lint
npm test
npm run build
```

See [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md) for CPA Usage Keeper
attribution and the preserved MIT License.
