use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError};

use serde::{Deserialize, Serialize};

use super::onepassword::{ItemIdCache, OnePasswordSecretStore, OpCli};
use crate::config::{Config, SecretsBackend};
use crate::error::ErrorClass;
use crate::ops::Secret;
use crate::paths::{Paths, write_private_atomic};

const KEYRING_SERVICE: &str = "rail";

/// Address used only to ask a secret store whether it is reachable. Nothing is ever stored
/// under it.
pub const PROBE_ADDRESS: &str = "rail-doctor-probe@invalid";

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TokenSet {
    pub access_token: String,
    pub refresh_token: String,
    /// Unix seconds.
    pub expires_at: i64,
}

/// Hides both tokens so that `{:?}` and tracing fields never leak credentials.
impl std::fmt::Debug for TokenSet {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TokenSet")
            .field("access_token", &"<redacted>")
            .field("refresh_token", &"<redacted>")
            .field("expires_at", &self.expires_at)
            .finish()
    }
}

#[derive(Debug, thiserror::Error)]
pub enum SecretError {
    #[error(
        "keyring unavailable: {0}. Set `secrets = \"file\"` under [general] in config.toml to keep tokens in a private file instead"
    )]
    KeyringUnavailable(String),
    #[error("keyring error: {0}")]
    Keyring(String),
    #[error("secrets file {}: {source}", .path.display())]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("secrets file {} is corrupt: {message}", .path.display())]
    Corrupt { path: PathBuf, message: String },
    #[error("token data is corrupt: {0}")]
    Encoding(String),
    #[error(
        "1Password is locked or not signed in ({0}); unlock the 1Password app or run `op signin`, then try again"
    )]
    OnePasswordLocked(String),
    #[error(
        "1Password did not answer in time; if it is waiting for you to unlock it, unlock it and try again"
    )]
    OnePasswordTimeout,
    #[error(
        "cannot run the 1Password CLI {program:?} ({message}); install it, or set op_path under [onepassword]"
    )]
    OnePasswordMissing { program: String, message: String },
    #[error(
        "refusing to run the 1Password CLI at {path}: {reason}; fix op_path under [onepassword]"
    )]
    OnePasswordUnsafe { path: String, reason: String },
    #[error("1Password has no such item: {0}")]
    OnePasswordNoItem(String),
    #[error("1Password: {0}")]
    OnePassword(String),
    /// `op` wrote more than the cap on one stream; nothing of it is kept.
    #[error("op's output exceeded 1 MiB")]
    OnePasswordOutputTooLarge,
    /// Rail's own complaint about a request or setting (no vault, a malformed reference).
    #[error("{0}")]
    OnePasswordInvalid(String),
    /// Moving bytes to or from `op` failed: `what` is a fixed phrase, `message` the OS text.
    #[error("could not {what} the 1Password CLI ({message})")]
    OnePasswordIo { what: &'static str, message: String },
    /// `op item edit` answered success, but reading the item back gave other values.
    #[error(
        "1Password did not keep the new tokens: reading the item back after the edit gave other values"
    )]
    OnePasswordEditUnverified,
}

impl SecretError {
    /// Filesystem and keyring hiccups may clear up; a missing keyring or a corrupt store will not.
    pub fn class(&self) -> ErrorClass {
        match self {
            Self::Io { .. }
            | Self::Keyring(_)
            | Self::OnePasswordTimeout
            | Self::OnePasswordIo { .. } => ErrorClass::Transient,
            Self::KeyringUnavailable(_)
            | Self::Corrupt { .. }
            | Self::Encoding(_)
            | Self::OnePasswordLocked(_)
            | Self::OnePasswordMissing { .. }
            | Self::OnePasswordUnsafe { .. }
            | Self::OnePasswordNoItem(_)
            | Self::OnePasswordInvalid(_)
            | Self::OnePasswordOutputTooLarge
            | Self::OnePasswordEditUnverified
            | Self::OnePassword(_) => ErrorClass::Permanent,
        }
    }
}

pub trait SecretStore: Send + Sync {
    fn get_tokens(&self, address: &str) -> Result<Option<TokenSet>, SecretError>;
    fn set_tokens(&self, address: &str, tokens: &TokenSet) -> Result<(), SecretError>;
    fn delete_tokens(&self, address: &str) -> Result<(), SecretError>;
    fn backend_name(&self) -> &'static str;

    fn get_password(&self, _key: &str) -> Result<Option<Secret<String>>, SecretError> {
        Err(SecretError::Encoding("get_password not implemented".into()))
    }

    fn set_password(&self, _key: &str, _password: &Secret<String>) -> Result<(), SecretError> {
        Err(SecretError::Encoding("set_password not implemented".into()))
    }

    fn delete_password(&self, _key: &str) -> Result<(), SecretError> {
        Err(SecretError::Encoding(
            "delete_password not implemented".into(),
        ))
    }

    fn has_password(&self, key: &str) -> Result<bool, SecretError> {
        self.get_password(key).map(|p| p.is_some())
    }

    /// Whether `address` has tokens. Stores that can answer without reading the tokens
    /// override this; the default reads them.
    fn has_tokens(&self, address: &str) -> Result<bool, SecretError> {
        self.get_tokens(address).map(|t| t.is_some())
    }

    /// The identity of the item holding `address`'s tokens, by a fresh lookup that reveals
    /// nothing; `None` for stores that are not made of items (or when there is none).
    fn item_id(&self, _address: &str) -> Result<Option<String>, SecretError> {
        Ok(None)
    }

    /// Check the store can be reached without changing anything.
    fn probe(&self) -> Result<(), SecretError> {
        self.get_tokens(PROBE_ADDRESS).map(|_| ())
    }
}

/// The store `config` selects. `ids` caches 1Password item ids across calls; the daemon keeps
/// one for its lifetime.
pub fn open_secret_store(
    config: &Config,
    paths: &Paths,
    ids: &ItemIdCache,
) -> Arc<dyn SecretStore> {
    match config.general.secrets {
        SecretsBackend::Keyring => Arc::new(KeyringSecretStore),
        SecretsBackend::File => Arc::new(FileSecretStore::new(paths.secrets_file())),
        SecretsBackend::OnePassword => {
            let op = config.onepassword.as_ref();
            Arc::new(OnePasswordSecretStore::new(
                OpCli::from_config(op, &paths.runtime_dir),
                op.map(|o| o.vault.clone()).unwrap_or_default(),
                ids.clone(),
            ))
        }
    }
}

/// Where `address`'s tokens live, for a person to read. Never the tokens.
pub fn token_location(config: &Config, paths: &Paths, address: &str) -> String {
    match config.general.secrets {
        SecretsBackend::Keyring => "OS keyring, service \"rail\"".to_string(),
        SecretsBackend::File => paths.secrets_file().display().to_string(),
        SecretsBackend::OnePassword => {
            let vault = config.onepassword.as_ref().map_or("", |o| o.vault.as_str());
            format!("1Password vault \"{vault}\", item \"Rail: {address}\"")
        }
    }
}

/// The secret-store key for an account password. The `pw:` namespace keeps passwords out of
/// the entries that hold OAuth tokens for the same address. `service` is `imap`, `smtp` or
/// `None` for the account's shared password.
pub fn password_key(address: &str, service: Option<&str>) -> String {
    match service.map(str::trim).filter(|s| !s.is_empty()) {
        Some(service) => format!("pw:{address}:{}", service.to_ascii_lowercase()),
        None => format!("pw:{address}"),
    }
}

/// Describe a JSON error by kind and position only. serde_json's own text
/// quotes the offending value, which here may be a token.
pub(crate) fn json_message(e: &serde_json::Error) -> String {
    let kind = match e.classify() {
        serde_json::error::Category::Io => "I/O error",
        serde_json::error::Category::Syntax => "syntax error",
        serde_json::error::Category::Data => "unexpected value",
        serde_json::error::Category::Eof => "unexpected end of data",
    };
    format!("{kind} at line {} column {}", e.line(), e.column())
}

fn encode(tokens: &TokenSet) -> Result<String, SecretError> {
    serde_json::to_string(tokens).map_err(|e| SecretError::Encoding(json_message(&e)))
}

fn decode(json: &str) -> Result<TokenSet, SecretError> {
    serde_json::from_str(json).map_err(|e| SecretError::Encoding(json_message(&e)))
}

// ---- OS keyring ----

pub struct KeyringSecretStore;

fn map_keyring(e: keyring::Error) -> SecretError {
    match e {
        keyring::Error::NoStorageAccess(inner) | keyring::Error::PlatformFailure(inner) => {
            SecretError::KeyringUnavailable(inner.to_string())
        }
        keyring::Error::NoDefaultStore => {
            SecretError::KeyringUnavailable("no credential store on this system".to_string())
        }
        other => SecretError::Keyring(other.to_string()),
    }
}

fn entry(address: &str) -> Result<keyring::Entry, SecretError> {
    keyring::Entry::new(KEYRING_SERVICE, address).map_err(map_keyring)
}

impl SecretStore for KeyringSecretStore {
    fn get_tokens(&self, address: &str) -> Result<Option<TokenSet>, SecretError> {
        match entry(address)?.get_password() {
            Ok(json) => decode(&json).map(Some),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(map_keyring(e)),
        }
    }

    fn set_tokens(&self, address: &str, tokens: &TokenSet) -> Result<(), SecretError> {
        entry(address)?
            .set_password(&encode(tokens)?)
            .map_err(map_keyring)
    }

    fn delete_tokens(&self, address: &str) -> Result<(), SecretError> {
        match entry(address)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(map_keyring(e)),
        }
    }

    fn get_password(&self, key: &str) -> Result<Option<Secret<String>>, SecretError> {
        match entry(key)?.get_password() {
            Ok(pass) => Ok(Some(Secret::new(pass))),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(map_keyring(e)),
        }
    }

    fn set_password(&self, key: &str, password: &Secret<String>) -> Result<(), SecretError> {
        entry(key)?
            .set_password(password.expose())
            .map_err(map_keyring)
    }

    fn delete_password(&self, key: &str) -> Result<(), SecretError> {
        match entry(key)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(map_keyring(e)),
        }
    }

    fn backend_name(&self) -> &'static str {
        "keyring"
    }
}

// ---- 0600 JSON file ----

#[derive(Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct FileSecrets {
    #[serde(default)]
    tokens: HashMap<String, TokenSet>,
    #[serde(default)]
    passwords: HashMap<String, String>,
}

pub struct FileSecretStore {
    path: PathBuf,
}

impl FileSecretStore {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    fn io_err(&self, source: std::io::Error) -> SecretError {
        SecretError::Io {
            path: self.path.clone(),
            source,
        }
    }

    fn read_all(&self) -> Result<FileSecrets, SecretError> {
        match std::fs::read_to_string(&self.path) {
            Ok(text) => {
                if let Ok(secrets) = serde_json::from_str::<FileSecrets>(&text) {
                    return Ok(secrets);
                }
                serde_json::from_str::<HashMap<String, TokenSet>>(&text)
                    .map(|tokens| FileSecrets {
                        tokens,
                        passwords: HashMap::new(),
                    })
                    .map_err(|e| SecretError::Corrupt {
                        path: self.path.clone(),
                        message: json_message(&e),
                    })
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(FileSecrets::default()),
            Err(e) => Err(self.io_err(e)),
        }
    }

    fn write_all(&self, secrets: &FileSecrets) -> Result<(), SecretError> {
        let json = serde_json::to_vec_pretty(secrets)
            .map_err(|e| SecretError::Encoding(json_message(&e)))?;
        write_private_atomic(&self.path, &json).map_err(|e| self.io_err(e))
    }
}

impl SecretStore for FileSecretStore {
    fn get_tokens(&self, address: &str) -> Result<Option<TokenSet>, SecretError> {
        Ok(self.read_all()?.tokens.remove(address))
    }

    fn set_tokens(&self, address: &str, tokens: &TokenSet) -> Result<(), SecretError> {
        let mut secrets = self.read_all()?;
        secrets.tokens.insert(address.to_string(), tokens.clone());
        self.write_all(&secrets)
    }

    fn delete_tokens(&self, address: &str) -> Result<(), SecretError> {
        let mut secrets = self.read_all()?;
        if secrets.tokens.remove(address).is_some() {
            self.write_all(&secrets)?;
        }
        Ok(())
    }

    fn get_password(&self, key: &str) -> Result<Option<Secret<String>>, SecretError> {
        Ok(self.read_all()?.passwords.remove(key).map(Secret::new))
    }

    fn set_password(&self, key: &str, password: &Secret<String>) -> Result<(), SecretError> {
        let mut secrets = self.read_all()?;
        secrets
            .passwords
            .insert(key.to_string(), password.expose().to_string());
        self.write_all(&secrets)
    }

    fn delete_password(&self, key: &str) -> Result<(), SecretError> {
        let mut secrets = self.read_all()?;
        if secrets.passwords.remove(key).is_some() {
            self.write_all(&secrets)?;
        }
        Ok(())
    }

    fn backend_name(&self) -> &'static str {
        "file"
    }
}

// ---- in-memory, for tests ----

#[derive(Default)]
pub struct MemorySecretStore {
    inner: Mutex<HashMap<String, TokenSet>>,
    passwords: Mutex<HashMap<String, Secret<String>>>,
}

impl SecretStore for MemorySecretStore {
    fn get_tokens(&self, address: &str) -> Result<Option<TokenSet>, SecretError> {
        Ok(self
            .inner
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(address)
            .cloned())
    }

    fn set_tokens(&self, address: &str, tokens: &TokenSet) -> Result<(), SecretError> {
        self.inner
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(address.to_string(), tokens.clone());
        Ok(())
    }

    fn delete_tokens(&self, address: &str) -> Result<(), SecretError> {
        self.inner
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(address);
        Ok(())
    }

    fn get_password(&self, key: &str) -> Result<Option<Secret<String>>, SecretError> {
        Ok(self
            .passwords
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(key)
            .cloned())
    }

    fn set_password(&self, key: &str, password: &Secret<String>) -> Result<(), SecretError> {
        self.passwords
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(key.to_string(), password.clone());
        Ok(())
    }

    fn delete_password(&self, key: &str) -> Result<(), SecretError> {
        self.passwords
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(key);
        Ok(())
    }

    fn backend_name(&self) -> &'static str {
        "memory"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tokens() -> TokenSet {
        TokenSet {
            access_token: "acc".to_string(),
            refresh_token: "ref".to_string(),
            expires_at: 1_700_000_000,
        }
    }

    #[test]
    fn memory_store_roundtrips() {
        let store = MemorySecretStore::default();
        assert_eq!(store.get_tokens("a@x.com").unwrap(), None);
        store.set_tokens("a@x.com", &tokens()).unwrap();
        assert_eq!(store.get_tokens("a@x.com").unwrap(), Some(tokens()));
        store.delete_tokens("a@x.com").unwrap();
        assert_eq!(store.get_tokens("a@x.com").unwrap(), None);
    }

    #[test]
    fn password_operations_roundtrip_in_memory_store() {
        let store = MemorySecretStore::default();
        let pass = Secret::new("hunter2");
        assert_eq!(store.get_password("a@x.com").unwrap(), None);
        store.set_password("a@x.com", &pass).unwrap();
        assert_eq!(
            store
                .get_password("a@x.com")
                .unwrap()
                .as_ref()
                .map(|s| s.expose()),
            Some("hunter2")
        );
        store.delete_password("a@x.com").unwrap();
        assert_eq!(store.get_password("a@x.com").unwrap(), None);
    }

    #[test]
    fn file_store_roundtrips_and_creates_parent_dir() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("deep").join("secrets.json");
        let store = FileSecretStore::new(path.clone());
        assert_eq!(store.get_tokens("a@x.com").unwrap(), None);
        store.set_tokens("a@x.com", &tokens()).unwrap();
        store.set_tokens("b@x.com", &tokens()).unwrap();
        assert_eq!(
            FileSecretStore::new(path.clone())
                .get_tokens("a@x.com")
                .unwrap(),
            Some(tokens())
        );
        store.delete_tokens("a@x.com").unwrap();
        assert_eq!(store.get_tokens("a@x.com").unwrap(), None);
        assert_eq!(store.get_tokens("b@x.com").unwrap(), Some(tokens()));
        assert!(!path.with_extension("json.tmp").exists());
    }

    #[test]
    fn file_store_passwords_roundtrip_and_coexist_with_tokens() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("secrets.json");
        let store = FileSecretStore::new(path.clone());
        let pass = Secret::new("imap-pass-123");
        assert_eq!(store.get_password("user@example.com").unwrap(), None);
        store.set_password("user@example.com", &pass).unwrap();
        store.set_tokens("oauth@example.com", &tokens()).unwrap();

        let reopened = FileSecretStore::new(path);
        assert_eq!(
            reopened
                .get_password("user@example.com")
                .unwrap()
                .as_ref()
                .map(|s| s.expose()),
            Some("imap-pass-123")
        );
        assert_eq!(
            reopened.get_tokens("oauth@example.com").unwrap(),
            Some(tokens())
        );

        reopened.delete_password("user@example.com").unwrap();
        assert_eq!(reopened.get_password("user@example.com").unwrap(), None);
        assert_eq!(
            reopened.get_tokens("oauth@example.com").unwrap(),
            Some(tokens())
        );
    }

    #[test]
    fn secret_debug_and_display_redact() {
        let pass = Secret::new("topsecret123");
        assert_eq!(format!("{pass:?}"), "Secret(<redacted>)");
        assert_eq!(format!("{pass}"), "<redacted>");
        assert_eq!(pass.expose(), "topsecret123");
    }

    #[cfg(unix)]
    #[test]
    fn file_store_is_private() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("secrets.json");
        FileSecretStore::new(path.clone())
            .set_tokens("a@x.com", &tokens())
            .unwrap();
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }

    #[cfg(unix)]
    #[test]
    fn file_store_creates_its_directory_private() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let data_dir = dir.path().join("data");
        FileSecretStore::new(data_dir.join("secrets.json"))
            .set_tokens("a@x.com", &tokens())
            .unwrap();
        let mode = std::fs::metadata(&data_dir).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o700);
    }

    #[test]
    fn corrupt_file_is_reported_not_overwritten() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("secrets.json");
        std::fs::write(&path, "{not json").unwrap();
        let store = FileSecretStore::new(path.clone());
        let err = store.get_tokens("a@x.com").unwrap_err();
        assert!(matches!(err, SecretError::Corrupt { .. }), "{err}");
        assert!(store.set_tokens("a@x.com", &tokens()).is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "{not json");
    }

    #[test]
    fn corrupt_value_errors_never_echo_the_value() {
        const TOKEN: &str = "ya29.super-secret-access-token";
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("secrets.json");
        std::fs::write(&path, format!("{{\"a@x.com\": \"{TOKEN}\"}}")).unwrap();
        let err = FileSecretStore::new(path)
            .get_tokens("a@x.com")
            .unwrap_err();
        assert!(matches!(err, SecretError::Corrupt { .. }), "{err}");
        assert!(!err.to_string().contains(TOKEN), "{err}");
        assert!(!format!("{err:?}").contains(TOKEN), "{err:?}");
        assert!(err.to_string().contains("line 1"), "{err}");

        let err = decode(&format!("\"{TOKEN}\"")).unwrap_err();
        assert!(matches!(err, SecretError::Encoding(_)), "{err}");
        assert!(!err.to_string().contains(TOKEN), "{err}");
        assert!(!format!("{err:?}").contains(TOKEN), "{err:?}");
    }

    #[test]
    fn keyring_unavailable_error_names_file_fallback() {
        let err = map_keyring(keyring::Error::NoStorageAccess("no dbus".into()));
        assert!(matches!(err, SecretError::KeyringUnavailable(_)));
        assert!(err.to_string().contains("secrets = \"file\""), "{err}");
        let err = map_keyring(keyring::Error::NoDefaultStore);
        assert!(matches!(err, SecretError::KeyringUnavailable(_)));
    }

    #[test]
    fn open_secret_store_picks_backend() {
        let dir = tempfile::tempdir().unwrap();
        let paths = crate::paths::Paths::all_in(dir.path());
        let ids = ItemIdCache::default();
        let with = |secrets| Config {
            general: crate::config::General {
                secrets,
                ..Default::default()
            },
            onepassword: Some(crate::config::OnePasswordConfig {
                vault: "Rail".into(),
                ..Default::default()
            }),
            ..Config::default()
        };
        let name = |b| open_secret_store(&with(b), &paths, &ids).backend_name();
        assert_eq!(name(SecretsBackend::File), "file");
        assert_eq!(name(SecretsBackend::Keyring), "keyring");
        assert_eq!(name(SecretsBackend::OnePassword), "1password");
    }

    #[test]
    fn token_location_names_the_place_never_the_tokens() {
        let dir = tempfile::tempdir().unwrap();
        let paths = crate::paths::Paths::all_in(dir.path());
        let mut config = Config::default();
        assert!(token_location(&config, &paths, "a@x.com").contains("keyring"));
        config.general.secrets = SecretsBackend::OnePassword;
        config.onepassword = Some(crate::config::OnePasswordConfig {
            vault: "Rail".into(),
            ..Default::default()
        });
        assert_eq!(
            token_location(&config, &paths, "a@x.com"),
            "1Password vault \"Rail\", item \"Rail: a@x.com\""
        );
    }

    #[test]
    fn debug_redacts_tokens() {
        let output = format!("{:?}", tokens());
        assert!(output.contains("<redacted>"), "{output}");
        assert!(output.contains("1700000000"), "{output}");
        assert!(!output.contains("\"acc\""), "{output}");
        assert!(!output.contains("\"ref\""), "{output}");
    }

    #[test]
    fn class_splits_transient_from_permanent() {
        let io = SecretError::Io {
            path: PathBuf::from("secrets.json"),
            source: std::io::Error::other("disk"),
        };
        assert_eq!(io.class(), ErrorClass::Transient);
        assert_eq!(
            SecretError::Keyring("boom".into()).class(),
            ErrorClass::Transient
        );
        assert_eq!(
            SecretError::KeyringUnavailable("no dbus".into()).class(),
            ErrorClass::Permanent
        );
        let corrupt = SecretError::Corrupt {
            path: PathBuf::from("secrets.json"),
            message: "eof".into(),
        };
        assert_eq!(corrupt.class(), ErrorClass::Permanent);
        assert_eq!(
            SecretError::Encoding("bad".into()).class(),
            ErrorClass::Permanent
        );
        assert_eq!(
            SecretError::OnePasswordTimeout.class(),
            ErrorClass::Transient
        );
        assert_eq!(
            SecretError::OnePasswordLocked("x".into()).class(),
            ErrorClass::Permanent
        );
        let permanent = [
            SecretError::OnePasswordMissing {
                program: "op".into(),
                message: "x".into(),
            },
            SecretError::OnePasswordUnsafe {
                path: "/op".into(),
                reason: "x".into(),
            },
            SecretError::OnePasswordNoItem("x".into()),
            SecretError::OnePassword("x".into()),
            SecretError::OnePasswordInvalid("x".into()),
            SecretError::OnePasswordOutputTooLarge,
            SecretError::OnePasswordEditUnverified,
        ];
        assert_eq!(
            SecretError::OnePasswordIo {
                what: "read from",
                message: "x".into()
            }
            .class(),
            ErrorClass::Transient
        );
        assert!(permanent.iter().all(|e| e.class() == ErrorClass::Permanent));
    }

    /// Talks to the real OS keyring. Run with `cargo test -- --ignored keyring_live`.
    #[test]
    #[ignore]
    fn keyring_live_roundtrip() {
        let store = KeyringSecretStore;
        let address = "rail-test@example.invalid";
        store.set_tokens(address, &tokens()).unwrap();
        assert_eq!(store.get_tokens(address).unwrap(), Some(tokens()));
        store.delete_tokens(address).unwrap();
        assert_eq!(store.get_tokens(address).unwrap(), None);
    }
}
