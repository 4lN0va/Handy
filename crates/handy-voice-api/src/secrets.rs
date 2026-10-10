//! Secret handling.
//!
//! # Threat model this module addresses
//!
//! An API key must never reach a transcript export, a settings dump, or a log
//! line. Two mechanisms enforce that here:
//!
//! 1. [`SecretString`] is a newtype whose `Debug`/`Display` implementation is
//!    redacted, so a key cannot leak through `{:?}` formatting — the single most
//!    common way secrets escape into logs.
//! 2. Secrets are held in an OS credential store via [`SecretStore`], keyed by
//!    an account name. Settings persistence therefore only ever stores the
//!    *account name*, never the secret value.
//!
//! # On fallbacks
//!
//! Upstream analysis of a sibling project found a keyring that silently fell
//! back to base64-in-a-file while the UI still reported success. That is worse
//! than failing. Here, the in-memory store is for tests only, and
//! [`SecretStoreKind`] is recorded so the UI can state honestly where a key is
//! stored.

use crate::error::VoiceApiError;
use std::collections::HashMap;
use std::fmt;
use std::sync::Mutex;

/// A secret value that cannot be accidentally printed.
///
/// Deliberately does not implement `Serialize`, so it cannot be written into a
/// settings file or a transcript by a `serde` derive.
///
/// # Known limitation: no memory scrubbing
///
/// This type protects against *logging and serialization* leakage, which is the
/// realistic failure mode. It does **not** zero the heap buffer on drop: doing
/// that safely requires care (the allocator may already have copied the value,
/// and `String` can reallocate), and this crate is `#![forbid(unsafe_code)]`.
/// Where that property is required, wrap the value with the `zeroize` crate
/// rather than hand-rolling an `unsafe` write.
#[derive(Clone, PartialEq, Eq)]
pub struct SecretString(String);

impl SecretString {
    /// Wrap a secret value.
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// Borrow the secret. Named `expose` so every call site is greppable.
    pub fn expose(&self) -> &str {
        &self.0
    }

    /// Whether the secret is empty.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Length in bytes, safe to log.
    pub fn len(&self) -> usize {
        self.0.len()
    }
}

// Redacted formatting: this is the whole point of the newtype.
impl fmt::Debug for SecretString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SecretString(***redacted***)")
    }
}

impl fmt::Display for SecretString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("***redacted***")
    }
}

/// Where a secret actually lives, so the UI can be truthful about it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecretStoreKind {
    /// The operating system credential store (Keychain / Credential Manager /
    /// Secret Service).
    OsCredentialStore,
    /// Process memory only. Never persists; used by tests.
    InMemory,
}

impl SecretStoreKind {
    /// A short, translatable-ish description for display.
    pub fn describe(self) -> &'static str {
        match self {
            Self::OsCredentialStore => "operating system credential store",
            Self::InMemory => "memory only (not saved)",
        }
    }
}

/// Storage for API keys and other credentials.
pub trait SecretStore: Send + Sync {
    /// Which backing store this is.
    fn kind(&self) -> SecretStoreKind;

    /// Read a secret. `Ok(None)` means "no such account", not "empty".
    fn get(&self, account: &str) -> Result<Option<SecretString>, VoiceApiError>;

    /// Store a secret, replacing any existing value.
    fn set(&self, account: &str, secret: &SecretString) -> Result<(), VoiceApiError>;

    /// Remove a secret. Removing a missing account is not an error.
    fn delete(&self, account: &str) -> Result<(), VoiceApiError>;
}

/// An in-process store. **Tests only** — nothing here is persisted, and the UI
/// is told so via [`SecretStoreKind::InMemory`].
#[derive(Default)]
pub struct MemorySecretStore {
    values: Mutex<HashMap<String, String>>,
}

impl MemorySecretStore {
    /// Create an empty in-memory store.
    pub fn new() -> Self {
        Self::default()
    }
}

impl SecretStore for MemorySecretStore {
    fn kind(&self) -> SecretStoreKind {
        SecretStoreKind::InMemory
    }

    fn get(&self, account: &str) -> Result<Option<SecretString>, VoiceApiError> {
        let map = self.values.lock().map_err(|_| VoiceApiError::SecretStore {
            account: account.to_string(),
            message: "secret store lock poisoned".into(),
        })?;
        Ok(map.get(account).map(SecretString::new))
    }

    fn set(&self, account: &str, secret: &SecretString) -> Result<(), VoiceApiError> {
        let mut map = self.values.lock().map_err(|_| VoiceApiError::SecretStore {
            account: account.to_string(),
            message: "secret store lock poisoned".into(),
        })?;
        map.insert(account.to_string(), secret.expose().to_string());
        Ok(())
    }

    fn delete(&self, account: &str) -> Result<(), VoiceApiError> {
        let mut map = self.values.lock().map_err(|_| VoiceApiError::SecretStore {
            account: account.to_string(),
            message: "secret store lock poisoned".into(),
        })?;
        map.remove(account);
        Ok(())
    }
}

/// The account name under which a provider's key is stored.
///
/// Keeping this a pure function makes it testable and stops call sites from
/// inventing inconsistent names (which would orphan a stored key).
pub fn account_name(service: &str, provider_id: &str) -> String {
    let sanitize = |s: &str| {
        s.chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                    c
                } else {
                    '-'
                }
            })
            .collect::<String>()
    };
    format!("{}:{}", sanitize(service), sanitize(provider_id))
}

/// The service name used for all keys belonging to this application.
pub const SECRET_SERVICE: &str = "handy-voice-api";

/// OS credential-store backed implementation.
#[cfg(feature = "keyring")]
pub mod os_store {
    use super::*;

    /// Stores secrets in the platform credential store.
    ///
    /// macOS Keychain, Windows Credential Manager, and Linux Secret Service are
    /// all handled by the `keyring` crate.
    pub struct OsSecretStore {
        service: String,
    }

    impl OsSecretStore {
        /// Create a store for `service` (normally [`SECRET_SERVICE`]).
        pub fn new(service: impl Into<String>) -> Self {
            Self {
                service: service.into(),
            }
        }

        fn entry(&self, account: &str) -> Result<keyring::Entry, VoiceApiError> {
            keyring::Entry::new(&self.service, account).map_err(|e| VoiceApiError::SecretStore {
                account: account.to_string(),
                // The message from the OS is useful, and cannot contain the secret.
                message: e.to_string(),
            })
        }
    }

    impl Default for OsSecretStore {
        fn default() -> Self {
            Self::new(SECRET_SERVICE)
        }
    }

    impl SecretStore for OsSecretStore {
        fn kind(&self) -> SecretStoreKind {
            SecretStoreKind::OsCredentialStore
        }

        fn get(&self, account: &str) -> Result<Option<SecretString>, VoiceApiError> {
            match self.entry(account)?.get_password() {
                Ok(value) => Ok(Some(SecretString::new(value))),
                Err(keyring::Error::NoEntry) => Ok(None),
                Err(e) => Err(VoiceApiError::SecretStore {
                    account: account.to_string(),
                    message: e.to_string(),
                }),
            }
        }

        fn set(&self, account: &str, secret: &SecretString) -> Result<(), VoiceApiError> {
            self.entry(account)?
                .set_password(secret.expose())
                .map_err(|e| VoiceApiError::SecretStore {
                    account: account.to_string(),
                    message: e.to_string(),
                })
        }

        fn delete(&self, account: &str) -> Result<(), VoiceApiError> {
            match self.entry(account)?.delete_credential() {
                Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
                Err(e) => Err(VoiceApiError::SecretStore {
                    account: account.to_string(),
                    message: e.to_string(),
                }),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY: &str = "sk-live-super-secret-0123456789abcdef";

    #[test]
    fn debug_formatting_never_reveals_the_secret() {
        let s = SecretString::new(KEY);
        let debugged = format!("{s:?}");
        assert!(
            !debugged.contains(KEY),
            "Debug leaked the secret: {debugged}"
        );
        assert!(debugged.contains("redacted"));
    }

    #[test]
    fn display_formatting_never_reveals_the_secret() {
        let s = SecretString::new(KEY);
        let displayed = format!("{s}");
        assert!(!displayed.contains(KEY));
        assert_eq!(displayed, "***redacted***");
    }

    #[test]
    fn a_struct_deriving_debug_does_not_leak_a_wrapped_secret() {
        // Models the realistic accident: someone derives Debug on a config
        // struct that holds a SecretString and logs it.
        #[derive(Debug)]
        #[allow(dead_code)]
        struct Config {
            base_url: String,
            key: SecretString,
        }
        let cfg = Config {
            base_url: "https://api.example.test/v1".into(),
            key: SecretString::new(KEY),
        };
        let logged = format!("{cfg:?}");
        assert!(
            !logged.contains(KEY),
            "derived Debug leaked the secret: {logged}"
        );
        assert!(
            logged.contains("https://api.example.test/v1"),
            "non-secrets stay readable"
        );
    }

    #[test]
    fn expose_returns_the_real_value_for_actual_use() {
        let s = SecretString::new(KEY);
        assert_eq!(s.expose(), KEY);
        assert_eq!(s.len(), KEY.len());
        assert!(!s.is_empty());
    }

    #[test]
    fn memory_store_round_trips_and_deletes() {
        let store = MemorySecretStore::new();
        let account = account_name(SECRET_SERVICE, "custom");
        assert!(store.get(&account).unwrap().is_none());

        store.set(&account, &SecretString::new(KEY)).unwrap();
        assert_eq!(store.get(&account).unwrap().unwrap().expose(), KEY);

        store.delete(&account).unwrap();
        assert!(store.get(&account).unwrap().is_none());
    }

    #[test]
    fn deleting_a_missing_account_is_not_an_error() {
        let store = MemorySecretStore::new();
        store
            .delete("never-existed")
            .expect("delete must be idempotent");
    }

    #[test]
    fn memory_store_reports_that_it_is_memory_only() {
        // The UI relies on this to avoid claiming a key was saved when it wasn't.
        assert_eq!(MemorySecretStore::new().kind(), SecretStoreKind::InMemory);
        assert!(MemorySecretStore::new()
            .kind()
            .describe()
            .contains("not saved"));
    }

    #[test]
    fn account_names_are_stable_and_sanitized() {
        assert_eq!(
            account_name("handy-voice-api", "custom"),
            "handy-voice-api:custom"
        );
        // A provider id with separators or spaces must not create odd key names.
        assert_eq!(
            account_name("handy-voice-api", "my provider/v2"),
            "handy-voice-api:my-provider-v2"
        );
        // Same input, same output — otherwise a stored key becomes orphaned.
        assert_eq!(
            account_name(SECRET_SERVICE, "openai"),
            account_name(SECRET_SERVICE, "openai")
        );
    }

    #[test]
    fn in_memory_store_never_returns_a_secret_for_a_different_account() {
        let store = MemorySecretStore::new();
        store
            .set(
                &account_name(SECRET_SERVICE, "a"),
                &SecretString::new("secret-a"),
            )
            .unwrap();
        assert!(store
            .get(&account_name(SECRET_SERVICE, "b"))
            .unwrap()
            .is_none());
    }
}
