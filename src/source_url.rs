use ipnet::IpNet;
use reqwest::{
    dns::{Addrs, Name, Resolve, Resolving},
    redirect, Url,
};
use std::{
    collections::HashSet,
    env,
    error::Error,
    fmt,
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
    str::FromStr,
    sync::Arc,
};

pub(crate) const SOURCE_URL_ALLOWLIST_ENV: &str = "GATEWAY_SOURCE_URL_ALLOWLIST";
const MAX_REDIRECTS: usize = 5;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SourceUrlPolicyError {
    InvalidAllowlist,
    InvalidUrl,
    DisallowedTarget,
    DnsResolutionFailed,
    NoResolvedAddresses,
    RedirectBlocked,
}

impl SourceUrlPolicyError {
    pub(crate) fn is_policy_violation(self) -> bool {
        matches!(
            self,
            Self::DisallowedTarget | Self::RedirectBlocked | Self::InvalidUrl
        )
    }
}

impl fmt::Display for SourceUrlPolicyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::InvalidAllowlist => "source URL allowlist contains an invalid entry",
            Self::InvalidUrl => "source URL is invalid",
            Self::DisallowedTarget => "source URL target is blocked by server policy",
            Self::DnsResolutionFailed => "source URL DNS resolution failed",
            Self::NoResolvedAddresses => "source URL DNS resolution returned no addresses",
            Self::RedirectBlocked => "source URL redirect is blocked by server policy",
        })
    }
}

impl Error for SourceUrlPolicyError {}

#[derive(Clone, Debug, Default)]
pub(crate) struct SourceUrlPolicy {
    allowed_hosts: Arc<HashSet<String>>,
    allowed_ips: Arc<HashSet<IpAddr>>,
    allowed_networks: Arc<Vec<IpNet>>,
}

impl SourceUrlPolicy {
    pub(crate) fn from_env() -> Result<Self, SourceUrlPolicyError> {
        match env::var(SOURCE_URL_ALLOWLIST_ENV) {
            Ok(value) => Self::from_allowlist(&value),
            Err(env::VarError::NotPresent) => Ok(Self::default()),
            Err(env::VarError::NotUnicode(_)) => Err(SourceUrlPolicyError::InvalidAllowlist),
        }
    }

    pub(crate) fn from_allowlist(value: &str) -> Result<Self, SourceUrlPolicyError> {
        let mut allowed_hosts = HashSet::new();
        let mut allowed_ips = HashSet::new();
        let mut allowed_networks = Vec::new();

        for entry in value
            .split(',')
            .map(str::trim)
            .filter(|entry| !entry.is_empty())
        {
            if entry.contains('/') {
                allowed_networks.push(
                    IpNet::from_str(entry).map_err(|_| SourceUrlPolicyError::InvalidAllowlist)?,
                );
                continue;
            }
            if let Ok(ip) = IpAddr::from_str(entry) {
                allowed_ips.insert(ip);
                continue;
            }
            let candidate = Url::parse(&format!("http://{entry}/"))
                .map_err(|_| SourceUrlPolicyError::InvalidAllowlist)?;
            let Some(host) = candidate.host_str() else {
                return Err(SourceUrlPolicyError::InvalidAllowlist);
            };
            if candidate.port().is_some()
                || !candidate.username().is_empty()
                || candidate.password().is_some()
                || candidate.path() != "/"
                || candidate.query().is_some()
                || candidate.fragment().is_some()
                || entry.contains('*')
            {
                return Err(SourceUrlPolicyError::InvalidAllowlist);
            }
            allowed_hosts.insert(normalize_host(host));
        }

        Ok(Self {
            allowed_hosts: Arc::new(allowed_hosts),
            allowed_ips: Arc::new(allowed_ips),
            allowed_networks: Arc::new(allowed_networks),
        })
    }

    pub(crate) fn validate_base_url(&self, value: &str) -> Result<Url, SourceUrlPolicyError> {
        let url = Url::parse(value).map_err(|_| SourceUrlPolicyError::InvalidUrl)?;
        self.validate_url_common(&url)?;
        if url.query().is_some() || url.fragment().is_some() {
            return Err(SourceUrlPolicyError::InvalidUrl);
        }
        Ok(url)
    }

    pub(crate) fn validate_request_url(&self, url: &Url) -> Result<(), SourceUrlPolicyError> {
        self.validate_url_common(url)?;
        if url.fragment().is_some() {
            return Err(SourceUrlPolicyError::InvalidUrl);
        }
        Ok(())
    }

    pub(crate) fn parse_request_url(&self, value: &str) -> Result<Url, SourceUrlPolicyError> {
        let url = Url::parse(value).map_err(|_| SourceUrlPolicyError::InvalidUrl)?;
        self.validate_request_url(&url)?;
        Ok(url)
    }

    pub(crate) fn validate_resolved_addresses(
        &self,
        host: &str,
        addresses: &[SocketAddr],
    ) -> Result<(), SourceUrlPolicyError> {
        if addresses.is_empty() {
            return Err(SourceUrlPolicyError::NoResolvedAddresses);
        }
        let host = normalize_host(host);
        if is_metadata_hostname(&host) {
            return Err(SourceUrlPolicyError::DisallowedTarget);
        }
        let host_is_allowed = self.allowed_hosts.contains(&host);
        if addresses
            .iter()
            .all(|address| self.ip_is_allowed(address.ip(), host_is_allowed))
        {
            Ok(())
        } else {
            Err(SourceUrlPolicyError::DisallowedTarget)
        }
    }

    pub(crate) fn redirect_policy(self: &Arc<Self>) -> redirect::Policy {
        let policy = Arc::clone(self);
        redirect::Policy::custom(move |attempt| {
            let blocked = attempt.previous().len() > MAX_REDIRECTS
                || attempt.previous().iter().any(|url| url == attempt.url())
                || attempt
                    .previous()
                    .first()
                    .is_none_or(|initial| !same_origin(initial, attempt.url()))
                || policy.validate_request_url(attempt.url()).is_err();
            if blocked {
                attempt.error(SourceUrlPolicyError::RedirectBlocked)
            } else {
                attempt.follow()
            }
        })
    }

    pub(crate) fn dns_resolver(self: &Arc<Self>) -> PolicyDnsResolver {
        PolicyDnsResolver {
            policy: Arc::clone(self),
        }
    }

    fn validate_url_common(&self, url: &Url) -> Result<(), SourceUrlPolicyError> {
        if !matches!(url.scheme(), "http" | "https")
            || url.host_str().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.port() == Some(0)
        {
            return Err(SourceUrlPolicyError::InvalidUrl);
        }
        let host = normalize_host(url.host_str().expect("host checked above"));
        if is_metadata_hostname(&host) {
            return Err(SourceUrlPolicyError::DisallowedTarget);
        }
        if let Ok(ip) = IpAddr::from_str(&host) {
            self.validate_ip(ip, false)?;
        } else if is_local_hostname(&host) && !self.allowed_hosts.contains(&host) {
            return Err(SourceUrlPolicyError::DisallowedTarget);
        }
        Ok(())
    }

    fn validate_ip(&self, ip: IpAddr, host_is_allowed: bool) -> Result<(), SourceUrlPolicyError> {
        if self.ip_is_allowed(ip, host_is_allowed) {
            Ok(())
        } else {
            Err(SourceUrlPolicyError::DisallowedTarget)
        }
    }

    fn ip_is_allowed(&self, ip: IpAddr, host_is_allowed: bool) -> bool {
        if is_never_allowed_ip(ip) {
            return false;
        }
        is_public_ip(ip)
            || host_is_allowed
            || self.allowed_ips.contains(&ip)
            || self
                .allowed_networks
                .iter()
                .any(|network| network.contains(&ip))
    }
}

#[derive(Clone, Debug)]
pub(crate) struct PolicyDnsResolver {
    policy: Arc<SourceUrlPolicy>,
}

impl Resolve for PolicyDnsResolver {
    fn resolve(&self, name: Name) -> Resolving {
        let host = name.as_str().to_owned();
        let policy = Arc::clone(&self.policy);
        Box::pin(async move {
            let mut addresses = tokio::net::lookup_host((host.as_str(), 0))
                .await
                .map_err(|_| boxed_error(SourceUrlPolicyError::DnsResolutionFailed))?
                .collect::<Vec<_>>();
            addresses.sort_unstable();
            addresses.dedup();
            policy
                .validate_resolved_addresses(&host, &addresses)
                .map_err(boxed_error)?;
            Ok(Box::new(addresses.into_iter()) as Addrs)
        })
    }
}

pub(crate) fn reqwest_error_is_policy_violation(error: &reqwest::Error) -> bool {
    let mut current: Option<&(dyn Error + 'static)> = Some(error);
    while let Some(source) = current {
        if source
            .downcast_ref::<SourceUrlPolicyError>()
            .is_some_and(|error| error.is_policy_violation())
        {
            return true;
        }
        current = source.source();
    }
    false
}

#[cfg(any(test, feature = "test-support"))]
pub(crate) fn test_policy() -> Arc<SourceUrlPolicy> {
    Arc::new(
        SourceUrlPolicy::from_allowlist("localhost,127.0.0.1,::1")
            .expect("test source URL allowlist"),
    )
}

fn boxed_error(error: SourceUrlPolicyError) -> Box<dyn Error + Send + Sync> {
    Box::new(error)
}

fn normalize_host(host: &str) -> String {
    let host = host.trim_end_matches('.');
    host.strip_prefix('[')
        .and_then(|host| host.strip_suffix(']'))
        .unwrap_or(host)
        .to_ascii_lowercase()
}

fn is_local_hostname(host: &str) -> bool {
    host == "localhost"
        || host.ends_with(".localhost")
        || host.ends_with(".local")
        || host.ends_with(".internal")
        || host.ends_with(".home")
        || host.ends_with(".lan")
        || !host.contains('.')
}

fn is_metadata_hostname(host: &str) -> bool {
    matches!(
        host,
        "metadata.google.internal"
            | "metadata.aws.internal"
            | "instance-data.ec2.internal"
            | "metadata.azure.internal"
    )
}

fn is_metadata_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => matches!(
            ip.octets(),
            [169, 254, 169, 254] | [169, 254, 170, 2] | [169, 254, 170, 23] | [100, 100, 100, 200]
        ),
        IpAddr::V6(ip) => {
            ip.segments() == [0xfd00, 0x0ec2, 0, 0, 0, 0, 0, 0x0254]
                || mapped_ipv4(ip).is_some_and(|ip| is_metadata_ip(IpAddr::V4(ip)))
        }
    }
}

fn is_never_allowed_ip(ip: IpAddr) -> bool {
    if is_metadata_ip(ip) {
        return true;
    }
    match ip {
        IpAddr::V4(ip) => ip.is_unspecified() || ip.is_multicast() || ip.is_broadcast(),
        IpAddr::V6(ip) => {
            ip.is_unspecified()
                || ip.is_multicast()
                || mapped_ipv4(ip).is_some_and(|ip| is_never_allowed_ip(IpAddr::V4(ip)))
        }
    }
}

fn is_public_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => is_public_ipv4(ip),
        IpAddr::V6(ip) => mapped_ipv4(ip).map_or_else(|| is_public_ipv6(ip), is_public_ipv4),
    }
}

fn is_public_ipv4(ip: Ipv4Addr) -> bool {
    !(ip.is_unspecified()
        || ip.is_loopback()
        || ip.is_private()
        || ip.is_link_local()
        || ip.is_multicast()
        || ip.is_broadcast()
        || ip.is_documentation()
        || ipv4_in(ip, [0, 0, 0, 0], 8)
        || ipv4_in(ip, [100, 64, 0, 0], 10)
        || ipv4_in(ip, [192, 0, 0, 0], 24)
        || ipv4_in(ip, [192, 88, 99, 0], 24)
        || ipv4_in(ip, [198, 18, 0, 0], 15)
        || ipv4_in(ip, [240, 0, 0, 0], 4))
}

fn is_public_ipv6(ip: Ipv6Addr) -> bool {
    ipv6_in(ip, [0x2000, 0, 0, 0, 0, 0, 0, 0], 3)
        && !ipv6_in(ip, [0x2001, 0, 0, 0, 0, 0, 0, 0], 23)
        && !ipv6_in(ip, [0x2001, 0x0db8, 0, 0, 0, 0, 0, 0], 32)
        && !ipv6_in(ip, [0x2002, 0, 0, 0, 0, 0, 0, 0], 16)
}

fn mapped_ipv4(ip: Ipv6Addr) -> Option<Ipv4Addr> {
    let segments = ip.segments();
    (segments[..5] == [0, 0, 0, 0, 0] && segments[5] == 0xffff).then(|| {
        Ipv4Addr::new(
            (segments[6] >> 8) as u8,
            segments[6] as u8,
            (segments[7] >> 8) as u8,
            segments[7] as u8,
        )
    })
}

fn ipv4_in(ip: Ipv4Addr, network: [u8; 4], prefix: u32) -> bool {
    let address = u32::from_be_bytes(ip.octets());
    let network = u32::from_be_bytes(network);
    let mask = u32::MAX.checked_shl(32 - prefix).unwrap_or(0);
    address & mask == network & mask
}

fn ipv6_in(ip: Ipv6Addr, network: [u16; 8], prefix: u32) -> bool {
    let address = u128::from_be_bytes(ip.octets());
    let network = u128::from_be_bytes(Ipv6Addr::from(network).octets());
    let mask = u128::MAX.checked_shl(128 - prefix).unwrap_or(0);
    address & mask == network & mask
}

fn same_origin(left: &Url, right: &Url) -> bool {
    left.scheme() == right.scheme()
        && left.host_str() == right.host_str()
        && left.port_or_known_default() == right.port_or_known_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_policy_rejects_special_literal_addresses_and_hostnames() {
        let policy = SourceUrlPolicy::default();
        for value in [
            "http://127.0.0.1:8787",
            "http://2130706433",
            "http://0177.0.0.1",
            "http://0x7f.1",
            "http://10.0.0.1",
            "http://169.254.169.254/latest/meta-data",
            "http://100.100.100.200/latest/meta-data",
            "http://[::1]",
            "http://[fd00::1]",
            "http://[fe80::1]",
            "http://[::ffff:127.0.0.1]",
            "http://localhost:8787",
            "http://service.internal",
            "http://metadata.google.internal",
        ] {
            assert!(
                policy.validate_base_url(value).is_err(),
                "{value} must be blocked"
            );
        }
    }

    #[test]
    fn public_urls_are_valid_but_credentials_and_ambiguous_parts_are_not() {
        let policy = SourceUrlPolicy::default();
        assert!(policy
            .validate_base_url("https://api.example.com/v1")
            .is_ok());
        assert!(policy.validate_base_url("https://8.8.8.8/v1").is_ok());
        assert!(policy
            .validate_base_url("https://[2001:4860:4860::8888]/v1")
            .is_ok());
        for value in [
            "ftp://api.example.com",
            "https://secret@api.example.com",
            "https://api.example.com?token=secret",
            "https://api.example.com#fragment",
            "https://api.example.com:0",
        ] {
            assert!(policy.validate_base_url(value).is_err());
        }
    }

    #[test]
    fn explicit_hosts_ips_and_cidrs_allow_private_targets_but_never_metadata() {
        let policy = SourceUrlPolicy::from_allowlist(
            "localhost,private.example,127.0.0.1,10.20.0.0/16,fd12:3456::/32",
        )
        .unwrap();
        assert!(policy.validate_base_url("http://localhost:11434").is_ok());
        assert!(policy.validate_base_url("http://127.0.0.1:11434").is_ok());
        assert!(policy.validate_base_url("http://10.20.5.7:8080").is_ok());
        assert!(policy
            .validate_base_url("http://[fd12:3456::7]:8080")
            .is_ok());
        assert!(policy.validate_base_url("http://10.21.0.1").is_err());
        assert!(policy
            .validate_resolved_addresses("private.example", &[SocketAddr::from(([10, 0, 0, 8], 0))])
            .is_ok());
        assert!(policy
            .validate_resolved_addresses(
                "private.example",
                &[SocketAddr::from(([169, 254, 169, 254], 0))]
            )
            .is_err());
        assert!(
            SourceUrlPolicy::from_allowlist("169.254.0.0/16,metadata.google.internal")
                .unwrap()
                .validate_base_url("http://169.254.169.254")
                .is_err()
        );
        let overly_broad = SourceUrlPolicy::from_allowlist("0.0.0.0/0,::/0").unwrap();
        for value in [
            "http://0.0.0.0",
            "http://224.0.0.1",
            "http://255.255.255.255",
            "http://[::]",
            "http://[ff02::1]",
            "http://[::ffff:169.254.169.254]",
        ] {
            assert!(
                overly_broad.validate_base_url(value).is_err(),
                "{value} must never be allowlisted"
            );
        }
    }

    #[test]
    fn every_dns_answer_must_be_allowed_to_prevent_rebinding() {
        let policy = SourceUrlPolicy::default();
        assert!(policy
            .validate_resolved_addresses("api.example.com", &[SocketAddr::from(([8, 8, 8, 8], 0))])
            .is_ok());
        assert!(policy
            .validate_resolved_addresses(
                "api.example.com",
                &[
                    SocketAddr::from(([8, 8, 8, 8], 0)),
                    SocketAddr::from(([127, 0, 0, 1], 0)),
                ]
            )
            .is_err());
        assert!(policy
            .validate_resolved_addresses("api.example.com", &[])
            .is_err());
    }

    #[test]
    fn allowlist_rejects_wildcards_origins_and_invalid_networks() {
        for value in [
            "*.internal",
            "http://localhost",
            "localhost:8080",
            "10.0.0.0/99",
        ] {
            assert_eq!(
                SourceUrlPolicy::from_allowlist(value).unwrap_err(),
                SourceUrlPolicyError::InvalidAllowlist
            );
        }
    }

    #[test]
    fn redirects_must_remain_same_origin_and_safe() {
        let policy = Arc::new(SourceUrlPolicy::default());
        let initial = Url::parse("https://api.example.com/v1/start").unwrap();
        let same = Url::parse("https://api.example.com/v1/final").unwrap();
        let other = Url::parse("https://other.example.com/v1/final").unwrap();
        let metadata = Url::parse("http://169.254.169.254/latest").unwrap();
        assert!(same_origin(&initial, &same));
        assert!(!same_origin(&initial, &other));
        assert!(policy.validate_request_url(&same).is_ok());
        assert!(policy.validate_request_url(&metadata).is_err());
    }
}
