//! Explicitly requested provider credentials stored in the operating system's
//! secure vault. This library never serializes secrets or falls back to files.
//! A successful API request does not prove stronger process isolation: other
//! applications running as this OS user may have access to the same vault.

use std::fmt;
use thiserror::Error;
use uuid::Uuid;
use zeroize::Zeroizing;

/// Constant namespace; callers cannot select another application's secrets.
pub const SERVICE: &str = "org.constellation.desktop.provider-credentials.v1";
pub const MAX_SECRET_BYTES: usize = 16 * 1024;

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct ProviderKey(Uuid);

impl ProviderKey {
    pub fn new(id: Uuid) -> Result<Self, VaultError> {
        if id.is_nil() {
            return Err(VaultError::InvalidProvider);
        }
        Ok(Self(id))
    }

    pub fn id(self) -> Uuid {
        self.0
    }
}

impl fmt::Debug for ProviderKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ProviderKey([redacted-id])")
    }
}

/// Always zeroizes on drop and never includes its value in Debug output.
pub struct Secret(Zeroizing<String>);

impl Secret {
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Secret([redacted])")
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum VaultError {
    #[error("invalid provider identifier")]
    InvalidProvider,
    #[error("provider credential is empty or exceeds the limit")]
    InvalidSecret,
    #[error("OS credential store is unavailable or access was denied")]
    Unavailable,
    #[error("provider credential was not found in the OS credential store")]
    NotFound,
}

/// Stateless access to the OS account's credential vault. No reads or writes
/// occur until the user explicitly requests an operation through a host.
#[derive(Clone, Copy, Default, Debug)]
pub struct Vault;

impl Vault {
    /// Store one key only when a user has explicitly chosen persistence.
    /// A stale or inaccessible vault is an error; no plaintext fallback exists.
    #[cfg(any(target_os = "windows", target_os = "macos"))]
    pub fn store(self, id: ProviderKey, secret: &str) -> Result<(), VaultError> {
        if secret.is_empty()
            || secret.len() > MAX_SECRET_BYTES
            || secret.trim().is_empty()
            || secret
                .chars()
                .any(|c| c == '\0' || c == '\r' || c == '\n' || c.is_control())
        {
            return Err(VaultError::InvalidSecret);
        }
        self.entry(id)?
            .set_password(secret)
            .map_err(|_| VaultError::Unavailable)
    }

    #[cfg(any(target_os = "windows", target_os = "macos"))]
    pub fn load(self, id: ProviderKey) -> Result<Secret, VaultError> {
        let text = self
            .entry(id)?
            .get_password()
            .map_err(|error| match error {
                keyring::Error::NoEntry => VaultError::NotFound,
                _ => VaultError::Unavailable,
            })?;
        if text.is_empty() || text.len() > MAX_SECRET_BYTES {
            return Err(VaultError::InvalidSecret);
        }
        Ok(Secret(Zeroizing::new(text)))
    }

    #[cfg(any(target_os = "windows", target_os = "macos"))]
    pub fn delete(self, id: ProviderKey) -> Result<(), VaultError> {
        self.entry(id)?
            .delete_credential()
            .map_err(|error| match error {
                keyring::Error::NoEntry => VaultError::NotFound,
                _ => VaultError::Unavailable,
            })
    }

    #[cfg(any(target_os = "windows", target_os = "macos"))]
    fn entry(self, id: ProviderKey) -> Result<keyring::Entry, VaultError> {
        keyring::Entry::new(SERVICE, &id.id().to_string()).map_err(|_| VaultError::Unavailable)
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    pub fn store(self, _id: ProviderKey, secret: &str) -> Result<(), VaultError> {
        if secret.is_empty()
            || secret.len() > MAX_SECRET_BYTES
            || secret.trim().is_empty()
            || secret.chars().any(|c| c.is_control())
        {
            return Err(VaultError::InvalidSecret);
        }
        Err(VaultError::Unavailable)
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    pub fn load(self, _id: ProviderKey) -> Result<Secret, VaultError> {
        Err(VaultError::Unavailable)
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    pub fn delete(self, _id: ProviderKey) -> Result<(), VaultError> {
        Err(VaultError::Unavailable)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identifiers_and_secret_lengths_are_validated_before_touching_os() {
        assert_eq!(
            ProviderKey::new(Uuid::nil()).unwrap_err(),
            VaultError::InvalidProvider
        );
        let id = ProviderKey::new(Uuid::new_v4()).unwrap();
        for bad in ["", "\n", " ", "bad\nkey", "\0", "bad\rkey"] {
            assert_eq!(Vault.store(id, bad), Err(VaultError::InvalidSecret));
        }
        assert_eq!(
            Vault.store(id, &"x".repeat(MAX_SECRET_BYTES + 1)),
            Err(VaultError::InvalidSecret)
        );
    }

    #[test]
    fn secret_and_provider_debug_output_are_redacted() {
        let original = "sk-sensitive-credential-value".to_owned();
        let secret = Secret(Zeroizing::new(original.clone()));
        let id = ProviderKey::new(Uuid::new_v4()).unwrap();
        let output = format!(
            "{secret:?} {id:?} {error:?}",
            error = VaultError::Unavailable
        );
        assert!(!output.contains(&original));
        assert!(!output.contains(&id.id().to_string()));
        assert_eq!(secret.expose(), original);
    }

    #[test]
    fn error_types_are_only_generic_non_secret_information() {
        assert_eq!(
            format!("{}", VaultError::Unavailable),
            "OS credential store is unavailable or access was denied"
        );
        assert_eq!(
            format!("{}", VaultError::NotFound),
            "provider credential was not found in the OS credential store"
        );
    }

    /// Explicit opt-in manual integration test. Never touches the OS credential
    /// manager in ordinary CI: the test only runs when the host sets the exact
    /// environment flag. A random provider ID prevents collision with real keys,
    /// and a guard deletes the temporary entry even if assertions panic.
    #[cfg(any(target_os = "windows", target_os = "macos"))]
    #[test]
    fn optional_real_os_vault_roundtrip_is_isolated_and_cleaned_up() {
        if std::env::var("CONSTELLATION_TEST_OS_VAULT").as_deref() != Ok("1") {
            return;
        }
        struct Cleanup(ProviderKey);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                let _ = Vault.delete(self.0);
            }
        }
        let id = ProviderKey::new(Uuid::new_v4()).expect("fresh non-nil provider ID");
        let _cleanup = Cleanup(id);
        let fixture = format!("constellation-os-vault-fixture-{}", id.id());
        Vault
            .store(id, &fixture)
            .expect("OS vault must accept explicit test key");
        assert_eq!(Vault.load(id).expect("vault roundtrip").expose(), fixture);
        Vault.delete(id).expect("test key should be removable");
        assert!(matches!(Vault.load(id), Err(VaultError::NotFound)));
    }
}
