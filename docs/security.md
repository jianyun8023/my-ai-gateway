# Security configuration

## Provider Source URL policy

Provider `Source.base_url` values are denied by default when they target
loopback, private, link-local, multicast, unspecified, documentation, reserved,
or other non-public address ranges. The same policy is applied when a Source is
imported or changed, when a PostgreSQL runtime snapshot is loaded, and when the
gateway sends runtime, connection-test, or model-discovery requests.

The outbound HTTP client resolves DNS through the policy and rejects the whole
answer set when any address is disallowed. The validated addresses are returned
directly to the connector, so the connection does not perform an unchecked
second resolution. Environment HTTP proxies are disabled for Provider traffic,
and redirects are limited to five same-origin hops. Scheme, host, or effective
port changes are rejected.

### Explicit self-hosted allowlist

Set `GATEWAY_SOURCE_URL_ALLOWLIST` only when a trusted self-hosted Provider must
use a local or private address. The value is a comma-separated list of exact
hostnames, exact IP addresses, and IP CIDRs:

```bash
export GATEWAY_SOURCE_URL_ALLOWLIST='localhost,127.0.0.1,10.20.0.0/16,fd12:3456::/32'
```

Entries are host-only: URL schemes, ports, paths, credentials, and wildcard
hostnames are rejected. A hostname entry explicitly trusts non-public addresses
returned for that exact hostname. Prefer an exact IP or the narrowest practical
CIDR when possible.

Common cloud metadata targets remain blocked even when an allowlist entry would
otherwise contain them. This includes the AWS/Azure/GCP link-local metadata
addresses, Alibaba's `100.100.100.200`, AWS ECS credential endpoints, and the
well-known metadata hostnames enforced by the gateway. Unspecified, multicast,
and broadcast targets also cannot be enabled by an allowlist.

The allowlist is read from the server environment at startup. It is not part of
Source JSON, cannot be supplied by a downstream request, and must not contain
credentials. An invalid entry stops startup instead of being ignored.

Blocked admin writes return `validation_failed` or `source_url_blocked` without
echoing the rejected URL. Runtime and discovery failures likewise return stable
messages without Authorization headers, credentials, resolved private IPs, or
the internal target URL.
