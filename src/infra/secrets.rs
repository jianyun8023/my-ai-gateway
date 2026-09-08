//! Account credential resolution and application-layer envelope encryption.
//!
//! The control plane stores only a reference (`credential_env`) or an
//! authenticated envelope (`credential_ciphertext`).  This module is the one
//! place that turns either reference into a short-lived, zeroized lease for an
//! upstream request.  Envelope parsing deliberately does not expose any
//! plaintext in errors or debug output.

use base64::{engine::general_purpose, Engine as _};
use ring::{
    aead,
    rand::{SecureRandom, SystemRandom},
};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, env, fmt, str, sync::Arc};
use zeroize::{Zeroize, Zeroizing};

/// Primary environment variable containing the active credential master key.
pub(crate) const MASTER_KEY_ENV: &str = "GATEWAY_CREDENTIAL_MASTER_KEY";
/// Optional keyring in `version=value,version2=value2` or JSON-object form.
pub(crate) const MASTER_KEYS_ENV: &str = "GATEWAY_CREDENTIAL_MASTER_KEYS";
/// Version used when `MASTER_KEY_ENV` is set without a keyring.
pub(crate) const MASTER_KEY_VERSION_ENV: &str = "GATEWAY_CREDENTIAL_MASTER_KEY_VERSION";
/// Version selected for newly encrypted envelopes.
pub(crate) const ACTIVE_KEY_VERSION_ENV: &str = "GATEWAY_CREDENTIAL_ACTIVE_KEY_VERSION";

const ENVELOPE_PREFIX: &str = "gwenc";
const ENVELOPE_VERSION: &str = "v1";
const NONCE_LEN: usize = 12;
const MAX_ENVELOPE_LEN: usize = 16 * 1024 * 1024;
const MAX_KEY_VERSION_LEN: usize = 64;
const ACCOUNT_AAD_PREFIX: &str = "my-ai-gateway/account/v1";
const VIRTUAL_KEY_AAD_PREFIX: &str = "my-ai-gateway/virtual-key/v1";

/// A stable, non-secret error returned by the resolver.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum SecretResolverError {
    /// No usable credential reference was configured.
    CredentialUnavailable,
    /// Both supported references were configured, which is ambiguous.
    AmbiguousCredential,
    /// The master-key environment is missing while ciphertext is configured.
    MasterKeyUnavailable,
    /// The master-key environment cannot be parsed.
    InvalidMasterKeyConfig,
    /// The envelope is malformed or uses an unsupported version.
    InvalidCiphertext,
    /// Authentication failed (wrong key, AAD, or tampered ciphertext).
    DecryptionFailed,
    /// The decrypted value is not valid UTF-8 or is empty.
    InvalidPlaintext,
}

impl SecretResolverError {
    pub(crate) fn code(&self) -> &'static str {
        match self {
            Self::CredentialUnavailable => "credential_unavailable",
            Self::AmbiguousCredential => "credential_ambiguous",
            Self::MasterKeyUnavailable => "credential_master_key_unavailable",
            Self::InvalidMasterKeyConfig => "credential_master_key_invalid",
            Self::InvalidCiphertext => "credential_ciphertext_invalid",
            Self::DecryptionFailed => "credential_decryption_failed",
            Self::InvalidPlaintext => "credential_plaintext_invalid",
        }
    }

    /// A public message suitable for an API error or an audit row.  It never
    /// includes a key, ciphertext, account value, or environment value.
    pub(crate) fn public_message(&self) -> &'static str {
        match self {
            Self::CredentialUnavailable => "account credential is unavailable",
            Self::AmbiguousCredential => "account credential configuration is ambiguous",
            Self::MasterKeyUnavailable => "credential master key is unavailable",
            Self::InvalidMasterKeyConfig => "credential master key configuration is invalid",
            Self::InvalidCiphertext => "account credential ciphertext is invalid",
            Self::DecryptionFailed => "account credential could not be decrypted",
            Self::InvalidPlaintext => "account credential plaintext is invalid",
        }
    }
}

impl fmt::Display for SecretResolverError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.public_message())
    }
}

impl std::error::Error for SecretResolverError {}

/// A short-lived credential value.  Its backing bytes are zeroized on drop.
/// The type intentionally has no `Serialize` implementation.
pub(crate) struct SecretLease(Zeroizing<Vec<u8>>);

impl SecretLease {
    fn new(value: Vec<u8>) -> Result<Self, SecretResolverError> {
        if value.is_empty() || str::from_utf8(&value).is_err() {
            return Err(SecretResolverError::InvalidPlaintext);
        }
        Ok(Self(Zeroizing::new(value)))
    }

    pub(crate) fn as_str(&self) -> &str {
        // `new` validates UTF-8 and the bytes are never mutated afterwards.
        str::from_utf8(&self.0).expect("validated secret lease is UTF-8")
    }
}

impl AsRef<str> for SecretLease {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl fmt::Debug for SecretLease {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SecretLease(REDACTED)")
    }
}

impl Drop for SecretLease {
    fn drop(&mut self) {
        // `Zeroizing` already performs this operation.  Keep the explicit
        // call as a defense if its implementation changes in a future release.
        self.0.zeroize();
    }
}

#[derive(Clone)]
struct KeyRing {
    active_version: String,
    keys: BTreeMap<String, [u8; 32]>,
}

/// Resolves environment references and decrypts credential envelopes.
///
/// The resolver is immutable and cheap to clone.  A process should construct
/// one at startup and reuse it for runtime, connection tests, and discovery;
/// changing the key environment therefore requires an explicit reload or
/// process restart rather than silently changing the active key mid-request.
#[derive(Clone)]
pub(crate) struct SecretResolver {
    keyring: Arc<KeyRing>,
}

impl fmt::Debug for SecretResolver {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SecretResolver")
            .field("active_version", &self.keyring.active_version)
            .field(
                "key_versions",
                &self.keyring.keys.keys().collect::<Vec<_>>(),
            )
            .finish()
    }
}

impl Default for SecretResolver {
    fn default() -> Self {
        Self::empty()
    }
}

impl SecretResolver {
    /// Construct a resolver with no master key.  Environment references still
    /// work; ciphertext references fail closed with `MasterKeyUnavailable`.
    pub(crate) fn empty() -> Self {
        Self {
            keyring: Arc::new(KeyRing {
                active_version: "v1".to_owned(),
                keys: BTreeMap::new(),
            }),
        }
    }

    /// Construct a resolver from one master key.  The key is hashed to a
    /// fixed 256-bit AES key unless it is already exactly 32 bytes.
    #[cfg(test)]
    pub(crate) fn from_master_key(master_key: impl AsRef<[u8]>) -> Self {
        Self::from_keyring("v1", [("v1".to_owned(), master_key.as_ref().to_vec())])
            .expect("fixed v1 key version is valid")
    }

    /// Construct a resolver from a keyring.  `active_version` is used for new
    /// envelopes; all supplied versions remain available for decryption.
    pub(crate) fn from_keyring<I, V>(
        active_version: impl Into<String>,
        keys: I,
    ) -> Result<Self, SecretResolverError>
    where
        I: IntoIterator<Item = (V, Vec<u8>)>,
        V: Into<String>,
    {
        let active_version = validate_key_version(&active_version.into())?;
        let mut normalized = BTreeMap::new();
        for (version, value) in keys {
            let version = validate_key_version(&version.into())?;
            if value.is_empty() {
                return Err(SecretResolverError::InvalidMasterKeyConfig);
            }
            normalized.insert(version, derive_key(&value));
        }
        if !normalized.is_empty() && !normalized.contains_key(&active_version) {
            return Err(SecretResolverError::InvalidMasterKeyConfig);
        }
        Ok(Self {
            keyring: Arc::new(KeyRing {
                active_version,
                keys: normalized,
            }),
        })
    }

    /// Load the resolver from process environment.  The following aliases are
    /// accepted to ease deployment migration: `GATEWAY_SECRET_MASTER_KEY`,
    /// `GATEWAY_ENCRYPTION_KEY`, and `GATEWAY_SECRET_MASTER_KEYS`.
    pub(crate) fn from_env() -> Result<Self, SecretResolverError> {
        let keyring_raw = first_nonempty_env(&[
            MASTER_KEYS_ENV,
            "GATEWAY_SECRET_MASTER_KEYS",
            "GATEWAY_ENCRYPTION_KEYS",
        ]);
        let primary = first_nonempty_env(&[
            MASTER_KEY_ENV,
            "GATEWAY_SECRET_MASTER_KEY",
            "GATEWAY_ENCRYPTION_KEY",
        ]);
        let primary_version = first_nonempty_env(&[
            MASTER_KEY_VERSION_ENV,
            "GATEWAY_SECRET_MASTER_KEY_VERSION",
            "GATEWAY_ENCRYPTION_KEY_VERSION",
        ])
        .unwrap_or_else(|| "v1".to_owned());
        let active = first_nonempty_env(&[
            ACTIVE_KEY_VERSION_ENV,
            "GATEWAY_SECRET_ACTIVE_KEY_VERSION",
            "GATEWAY_ENCRYPTION_ACTIVE_KEY_VERSION",
        ]);

        let mut keys = Vec::<(String, Vec<u8>)>::new();
        if let Some(raw) = keyring_raw {
            keys.extend(parse_keyring(&raw)?);
        }
        if let Some(value) = primary {
            keys.push((primary_version, value.into_bytes()));
        }
        if keys.is_empty() {
            return Ok(Self::empty());
        }
        let active = active.unwrap_or_else(|| keys.last().map(|(v, _)| v.clone()).unwrap());
        Self::from_keyring(active, keys)
    }

    pub(crate) fn active_key_version(&self) -> &str {
        &self.keyring.active_version
    }

    pub(crate) fn has_master_key(&self) -> bool {
        !self.keyring.keys.is_empty()
    }

    /// Bind an envelope to a source/account identity.  The identity is public
    /// metadata, but binding it prevents ciphertext replay across accounts.
    pub(crate) fn account_aad(source_id: &str, account_id: &str) -> String {
        format!("{ACCOUNT_AAD_PREFIX}/{source_id}/{account_id}")
    }

    pub(crate) fn virtual_key_aad(key_prefix: &str) -> String {
        format!("{VIRTUAL_KEY_AAD_PREFIX}/{key_prefix}")
    }

    pub(crate) fn resolve_virtual_key(
        &self,
        key_prefix: &str,
        ciphertext: &str,
    ) -> Result<SecretLease, SecretResolverError> {
        let aad = Self::virtual_key_aad(key_prefix);
        self.resolve_refs(None, Some(ciphertext), None, aad.as_bytes())
    }

    /// Resolve an Account's references.  Exactly one of `credential_env`,
    /// `credential_ciphertext`, or the test-only inline value may be present.
    pub(crate) fn resolve_account(
        &self,
        source_id: &str,
        account_id: &str,
        credential_env: Option<&str>,
        credential_ciphertext: Option<&str>,
        inline_credential: Option<&str>,
    ) -> Result<SecretLease, SecretResolverError> {
        let aad = Self::account_aad(source_id, account_id);
        self.resolve_refs(
            credential_env,
            credential_ciphertext,
            inline_credential,
            aad.as_bytes(),
        )
    }

    /// Resolve references with explicit AAD.  This is useful for non-account
    /// callers and keeps the cryptographic primitive independently testable.
    pub(crate) fn resolve_refs(
        &self,
        credential_env: Option<&str>,
        credential_ciphertext: Option<&str>,
        inline_credential: Option<&str>,
        aad: &[u8],
    ) -> Result<SecretLease, SecretResolverError> {
        let env_name = credential_env.filter(|value| !value.trim().is_empty());
        let ciphertext = credential_ciphertext.filter(|value| !value.trim().is_empty());
        let inline = inline_credential.filter(|value| !value.is_empty());
        let configured = usize::from(env_name.is_some())
            + usize::from(ciphertext.is_some())
            + usize::from(inline.is_some());
        if configured == 0 {
            return Err(SecretResolverError::CredentialUnavailable);
        }
        if configured > 1 {
            return Err(SecretResolverError::AmbiguousCredential);
        }
        if let Some(name) = env_name {
            let value = env::var(name)
                .ok()
                .filter(|value| !value.is_empty())
                .ok_or(SecretResolverError::CredentialUnavailable)?;
            return SecretLease::new(value.into_bytes());
        }
        if let Some(value) = inline {
            return SecretLease::new(value.as_bytes().to_vec());
        }
        self.decrypt(ciphertext.expect("configured ciphertext"), aad)
    }

    /// Encrypt a value into the canonical `gwenc:v1:key:nonce:ciphertext`
    /// envelope.  This method is intended for provisioning/rotation tooling;
    /// request handlers should only call `resolve_*`.
    pub(crate) fn encrypt(
        &self,
        plaintext: &str,
        aad: &[u8],
    ) -> Result<String, SecretResolverError> {
        if plaintext.is_empty() {
            return Err(SecretResolverError::InvalidPlaintext);
        }
        let key_version = &self.keyring.active_version;
        let key = self
            .keyring
            .keys
            .get(key_version)
            .ok_or(SecretResolverError::MasterKeyUnavailable)?;
        let mut nonce_bytes = [0_u8; NONCE_LEN];
        SystemRandom::new()
            .fill(&mut nonce_bytes)
            .map_err(|_| SecretResolverError::MasterKeyUnavailable)?;
        let nonce = aead::Nonce::try_assume_unique_for_key(&nonce_bytes)
            .map_err(|_| SecretResolverError::InvalidCiphertext)?;
        let mut payload = plaintext.as_bytes().to_vec();
        let less_safe = less_safe_key(key)?;
        less_safe
            .seal_in_place_append_tag(nonce, aead::Aad::from(aad), &mut payload)
            .map_err(|_| SecretResolverError::DecryptionFailed)?;
        Ok(format!(
            "{ENVELOPE_PREFIX}:{ENVELOPE_VERSION}:{key_version}:{}:{}",
            general_purpose::URL_SAFE_NO_PAD.encode(nonce_bytes),
            general_purpose::URL_SAFE_NO_PAD.encode(payload)
        ))
    }

    /// Encrypt with account-bound AAD.
    pub(crate) fn encrypt_for_account(
        &self,
        source_id: &str,
        account_id: &str,
        plaintext: &str,
    ) -> Result<String, SecretResolverError> {
        let aad = Self::account_aad(source_id, account_id);
        self.encrypt(plaintext, aad.as_bytes())
    }

    /// Re-encrypt an envelope with the current active key version.  Existing
    /// key versions remain valid for decryption, so rotation can be gradual.
    pub(crate) fn rotate(
        &self,
        ciphertext: &str,
        aad: &[u8],
    ) -> Result<String, SecretResolverError> {
        let plaintext = self.decrypt(ciphertext, aad)?;
        self.encrypt(plaintext.as_str(), aad)
    }

    pub(crate) fn rotate_for_account(
        &self,
        source_id: &str,
        account_id: &str,
        ciphertext: &str,
    ) -> Result<String, SecretResolverError> {
        let aad = Self::account_aad(source_id, account_id);
        self.rotate(ciphertext, aad.as_bytes())
    }

    #[cfg(test)]
    pub(crate) fn needs_rotation(&self, ciphertext: &str) -> Result<bool, SecretResolverError> {
        let envelope = parse_envelope(ciphertext)?;
        Ok(envelope.key_version != self.keyring.active_version)
    }

    /// Validate envelope structure without attempting decryption.  Control
    /// plane writes use this to reject accidental plaintext while still
    /// allowing ciphertext encrypted by a previous key version.
    #[cfg(test)]
    pub(crate) fn validate_ciphertext(ciphertext: &str) -> Result<(), SecretResolverError> {
        parse_envelope(ciphertext).map(|_| ())
    }

    fn decrypt(&self, ciphertext: &str, aad: &[u8]) -> Result<SecretLease, SecretResolverError> {
        let envelope = parse_envelope(ciphertext)?;
        let key = self
            .keyring
            .keys
            .get(&envelope.key_version)
            .ok_or(SecretResolverError::MasterKeyUnavailable)?;
        let nonce = aead::Nonce::try_assume_unique_for_key(&envelope.nonce)
            .map_err(|_| SecretResolverError::InvalidCiphertext)?;
        let less_safe = less_safe_key(key)?;
        let mut payload = envelope.ciphertext;
        let plaintext = less_safe
            .open_in_place(nonce, aead::Aad::from(aad), &mut payload)
            .map_err(|_| SecretResolverError::DecryptionFailed)?
            .to_vec();
        SecretLease::new(plaintext)
    }
}

#[derive(Debug)]
struct Envelope {
    key_version: String,
    nonce: [u8; NONCE_LEN],
    ciphertext: Vec<u8>,
}

fn less_safe_key(key: &[u8; 32]) -> Result<aead::LessSafeKey, SecretResolverError> {
    let unbound = aead::UnboundKey::new(&aead::AES_256_GCM, key)
        .map_err(|_| SecretResolverError::InvalidMasterKeyConfig)?;
    Ok(aead::LessSafeKey::new(unbound))
}

fn derive_key(value: &[u8]) -> [u8; 32] {
    let decoded = decode_key_material(value).unwrap_or_else(|| value.to_vec());
    if decoded.len() == 32 {
        decoded.try_into().expect("length checked")
    } else {
        Sha256::digest(decoded).into()
    }
}

fn decode_key_material(value: &[u8]) -> Option<Vec<u8>> {
    let value = str::from_utf8(value).ok()?;
    if let Some(encoded) = value.strip_prefix("base64:") {
        return general_purpose::STANDARD
            .decode(encoded)
            .or_else(|_| general_purpose::URL_SAFE_NO_PAD.decode(encoded))
            .ok();
    }
    if let Some(encoded) = value.strip_prefix("hex:") {
        if encoded.len() % 2 != 0 {
            return None;
        }
        return (0..encoded.len())
            .step_by(2)
            .map(|index| u8::from_str_radix(&encoded[index..index + 2], 16).ok())
            .collect();
    }
    None
}

fn validate_key_version(value: &str) -> Result<String, SecretResolverError> {
    if value.is_empty()
        || value.len() > MAX_KEY_VERSION_LEN
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
    {
        return Err(SecretResolverError::InvalidMasterKeyConfig);
    }
    Ok(value.to_owned())
}

fn first_nonempty_env(names: &[&str]) -> Option<String> {
    names
        .iter()
        .find_map(|name| env::var(name).ok().filter(|value| !value.trim().is_empty()))
}

fn parse_keyring(raw: &str) -> Result<Vec<(String, Vec<u8>)>, SecretResolverError> {
    let raw = raw.trim();
    if raw.starts_with('{') {
        let object: serde_json::Map<String, Value> =
            serde_json::from_str(raw).map_err(|_| SecretResolverError::InvalidMasterKeyConfig)?;
        let mut entries = Vec::with_capacity(object.len());
        for (version, value) in object {
            let value = value
                .as_str()
                .ok_or(SecretResolverError::InvalidMasterKeyConfig)?;
            entries.push((version, value.as_bytes().to_vec()));
        }
        return Ok(entries);
    }
    raw.split([',', ';'])
        .filter(|entry| !entry.trim().is_empty())
        .map(|entry| {
            let (version, value) = entry
                .split_once('=')
                .ok_or(SecretResolverError::InvalidMasterKeyConfig)?;
            if value.is_empty() {
                return Err(SecretResolverError::InvalidMasterKeyConfig);
            }
            Ok((version.trim().to_owned(), value.to_owned().into_bytes()))
        })
        .collect()
}

fn parse_envelope(value: &str) -> Result<Envelope, SecretResolverError> {
    if value.len() > MAX_ENVELOPE_LEN || value.trim().is_empty() {
        return Err(SecretResolverError::InvalidCiphertext);
    }
    let value = value.trim();
    if value.starts_with('{') {
        return parse_json_envelope(value);
    }
    let mut fields = if value.contains(':') {
        value.split(':').collect::<Vec<_>>()
    } else {
        value.split('.').collect::<Vec<_>>()
    };
    // Canonical: gwenc:v1:key:nonce:ciphertext.  Accept v1:key:... and
    // enc:v1:key:... to make migration from early development snapshots safe.
    if fields.len() == 5 && (fields[0] == ENVELOPE_PREFIX || fields[0] == "enc") {
        fields.remove(0);
    }
    if fields.len() != 4 || fields[0] != ENVELOPE_VERSION {
        return Err(SecretResolverError::InvalidCiphertext);
    }
    let key_version = validate_key_version(fields[1])?;
    let nonce = decode_nonce(fields[2])?;
    let ciphertext = decode_payload(fields[3])?;
    if ciphertext.len() < aead::AES_256_GCM.tag_len() {
        return Err(SecretResolverError::InvalidCiphertext);
    }
    Ok(Envelope {
        key_version,
        nonce,
        ciphertext,
    })
}

fn parse_json_envelope(value: &str) -> Result<Envelope, SecretResolverError> {
    let object: serde_json::Map<String, Value> =
        serde_json::from_str(value).map_err(|_| SecretResolverError::InvalidCiphertext)?;
    let version = object
        .get("version")
        .and_then(|value| {
            value
                .as_str()
                .map(ToOwned::to_owned)
                .or_else(|| value.as_u64().map(|v| format!("v{v}")))
        })
        .ok_or(SecretResolverError::InvalidCiphertext)?;
    let key_version = object
        .get("key_version")
        .or_else(|| object.get("key_id"))
        .and_then(Value::as_str)
        .ok_or(SecretResolverError::InvalidCiphertext)?;
    if version != ENVELOPE_VERSION {
        return Err(SecretResolverError::InvalidCiphertext);
    }
    let nonce = object
        .get("nonce")
        .and_then(Value::as_str)
        .ok_or(SecretResolverError::InvalidCiphertext)?;
    let ciphertext = object
        .get("ciphertext")
        .and_then(Value::as_str)
        .ok_or(SecretResolverError::InvalidCiphertext)?;
    let nonce = decode_nonce(nonce)?;
    let ciphertext = decode_payload(ciphertext)?;
    if ciphertext.len() < aead::AES_256_GCM.tag_len() {
        return Err(SecretResolverError::InvalidCiphertext);
    }
    Ok(Envelope {
        key_version: validate_key_version(key_version)?,
        nonce,
        ciphertext,
    })
}

fn decode_nonce(value: &str) -> Result<[u8; NONCE_LEN], SecretResolverError> {
    let bytes = decode_b64(value)?;
    bytes
        .try_into()
        .map_err(|_| SecretResolverError::InvalidCiphertext)
}

fn decode_payload(value: &str) -> Result<Vec<u8>, SecretResolverError> {
    let bytes = decode_b64(value)?;
    if bytes.len() > MAX_ENVELOPE_LEN {
        return Err(SecretResolverError::InvalidCiphertext);
    }
    Ok(bytes)
}

fn decode_b64(value: &str) -> Result<Vec<u8>, SecretResolverError> {
    general_purpose::URL_SAFE_NO_PAD
        .decode(value)
        .or_else(|_| general_purpose::STANDARD.decode(value))
        .map_err(|_| SecretResolverError::InvalidCiphertext)
}

impl SecretResolver {
    /// Resolve an account credential without collapsing resolver failures.
    /// A genuinely unconfigured credential remains `Ok(None)` because some
    /// upstreams do not require authentication; malformed or undecryptable
    /// configured credentials are returned to callers for safe eventing.
    pub(crate) fn resolve_account_credential_result(
        &self,
        account: &crate::domain::config::AccountConfig,
    ) -> Result<Option<String>, SecretResolverError> {
        let credential_configured = account
            .credential_env
            .as_deref()
            .is_some_and(|value| !value.trim().is_empty())
            || account
                .credential_ciphertext
                .as_deref()
                .is_some_and(|value| !value.trim().is_empty())
            || account
                .credential
                .as_deref()
                .is_some_and(|value| !value.is_empty());
        match self.resolve_account(
            &account.provider_id,
            &account.id,
            account.credential_env.as_deref(),
            account.credential_ciphertext.as_deref(),
            account.credential.as_deref(),
        ) {
            Ok(lease) => Ok(Some(lease.as_str().to_owned())),
            Err(SecretResolverError::CredentialUnavailable) if !credential_configured => Ok(None),
            Err(error) => Err(error),
        }
    }

    #[cfg(test)]
    pub(crate) fn resolve_account_credential(
        &self,
        account: &crate::domain::config::AccountConfig,
    ) -> Option<String> {
        match self.resolve_account_credential_result(account) {
            Ok(credential) => credential,
            Err(err) => {
                tracing::warn!(
                    account_id = %account.id,
                    error_code = err.code(),
                    "credential resolution failed"
                );
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::config::AccountConfig;
    use std::sync::Mutex;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn environment_reference_resolves_without_master_key() {
        let _guard = ENV_LOCK.lock().unwrap();
        let name = "SECRET_RESOLVER_ENV_TEST";
        std::env::set_var(name, "env-secret");
        let value = SecretResolver::empty()
            .resolve_refs(Some(name), None, None, b"aad")
            .unwrap();
        assert_eq!(value.as_str(), "env-secret");
        std::env::remove_var(name);
    }

    #[test]
    fn envelope_round_trip_binds_aad_and_never_displays_plaintext() {
        let resolver = SecretResolver::from_master_key("master-for-test");
        let ciphertext = resolver.encrypt("cipher-secret", b"account-aad").unwrap();
        assert!(ciphertext.starts_with("gwenc:v1:v1:"));
        let value = resolver
            .resolve_refs(None, Some(&ciphertext), None, b"account-aad")
            .unwrap();
        assert_eq!(value.as_str(), "cipher-secret");
        assert!(!format!("{resolver:?}").contains("cipher-secret"));
        assert!(!format!("{value:?}").contains("cipher-secret"));
        assert!(!resolver.needs_rotation(&ciphertext).unwrap());
    }

    #[test]
    fn virtual_key_recovery_is_bound_to_its_public_prefix() {
        let resolver = SecretResolver::from_master_key("virtual-key-master-for-test");
        let prefix = "mgk_example";
        let aad = SecretResolver::virtual_key_aad(prefix);
        let ciphertext = resolver
            .encrypt("mgk_example-secret", aad.as_bytes())
            .unwrap();

        assert_eq!(
            resolver
                .resolve_virtual_key(prefix, &ciphertext)
                .unwrap()
                .as_str(),
            "mgk_example-secret"
        );
        assert_eq!(
            resolver
                .resolve_virtual_key("mgk_other", &ciphertext)
                .unwrap_err(),
            SecretResolverError::DecryptionFailed
        );
    }

    #[test]
    fn wrong_key_and_wrong_aad_fail_closed_without_secret_in_errors() {
        let resolver = SecretResolver::from_master_key("correct-master");
        let ciphertext = resolver.encrypt("never-log-me", b"aad").unwrap();
        let wrong_key = SecretResolver::from_master_key("wrong-master");
        let error = wrong_key
            .resolve_refs(None, Some(&ciphertext), None, b"aad")
            .unwrap_err();
        assert_eq!(error, SecretResolverError::DecryptionFailed);
        assert!(!error.to_string().contains("never-log-me"));
        let error = resolver
            .resolve_refs(None, Some(&ciphertext), None, b"other-aad")
            .unwrap_err();
        assert_eq!(error, SecretResolverError::DecryptionFailed);
    }

    #[test]
    fn rotation_uses_active_key_and_keeps_previous_key_readable() {
        let old = SecretResolver::from_keyring("old", [("old", b"old-master".to_vec())]).unwrap();
        let original = old.encrypt("rotate-me", b"aad").unwrap();
        let rotated = SecretResolver::from_keyring(
            "new",
            [
                ("old", b"old-master".to_vec()),
                ("new", b"new-master".to_vec()),
            ],
        )
        .unwrap();
        assert!(rotated.needs_rotation(&original).unwrap());
        let current = rotated.rotate(&original, b"aad").unwrap();
        assert!(current.starts_with("gwenc:v1:new:"));
        assert!(!rotated.needs_rotation(&current).unwrap());
        assert_eq!(
            rotated
                .resolve_refs(None, Some(&current), None, b"aad")
                .unwrap()
                .as_str(),
            "rotate-me"
        );
    }

    #[test]
    fn malformed_and_ambiguous_references_are_rejected() {
        let resolver = SecretResolver::empty();
        assert_eq!(
            resolver
                .resolve_refs(Some("MISSING"), Some("v1:v1:a:b"), None, b"aad")
                .unwrap_err(),
            SecretResolverError::AmbiguousCredential
        );
        assert_eq!(
            SecretResolver::validate_ciphertext("plain-secret").unwrap_err(),
            SecretResolverError::InvalidCiphertext
        );
    }

    #[test]
    fn configured_but_missing_environment_credential_remains_an_error() {
        let _guard = ENV_LOCK.lock().unwrap();
        let name = "SECRET_RESOLVER_MISSING_ACCOUNT_ENV_TEST";
        std::env::remove_var(name);
        let resolver = SecretResolver::empty();
        let mut account = AccountConfig {
            id: "account-a".into(),
            provider_id: "source-a".into(),
            display_name: "Account A".into(),
            credential_env: Some(name.into()),
            credential_ciphertext: None,
            credential: None,
            enabled: true,
            weight: 1,
            protocol_capabilities: Default::default(),
            capabilities: None,
            model_overrides: Default::default(),
            model_map: Default::default(),
        };

        assert_eq!(
            resolver.resolve_account_credential_result(&account),
            Err(SecretResolverError::CredentialUnavailable)
        );
        account.credential_env = None;
        assert_eq!(
            resolver.resolve_account_credential_result(&account),
            Ok(None)
        );
    }

    #[test]
    fn keyring_environment_accepts_json_and_explicit_active_version() {
        let _guard = ENV_LOCK.lock().unwrap();
        let old = std::env::var_os(MASTER_KEYS_ENV);
        let old_active = std::env::var_os(ACTIVE_KEY_VERSION_ENV);
        let old_primary = std::env::var_os(MASTER_KEY_ENV);
        std::env::set_var(
            MASTER_KEYS_ENV,
            r#"{"old":"old-master","new":"new-master"}"#,
        );
        std::env::set_var(ACTIVE_KEY_VERSION_ENV, "new");
        std::env::remove_var(MASTER_KEY_ENV);
        let resolver = SecretResolver::from_env().unwrap();
        assert_eq!(resolver.active_key_version(), "new");
        let ciphertext = resolver.encrypt("env-rotation", b"aad").unwrap();
        assert_eq!(
            resolver
                .resolve_refs(None, Some(&ciphertext), None, b"aad")
                .unwrap()
                .as_str(),
            "env-rotation"
        );
        restore_env(MASTER_KEYS_ENV, old);
        restore_env(ACTIVE_KEY_VERSION_ENV, old_active);
        restore_env(MASTER_KEY_ENV, old_primary);
    }

    fn restore_env(name: &str, value: Option<std::ffi::OsString>) {
        match value {
            Some(value) => std::env::set_var(name, value),
            None => std::env::remove_var(name),
        }
    }
}
