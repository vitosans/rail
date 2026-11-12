//! The 1Password backend: one item per mail account in a vault, titled `Rail: <address>` and
//! tagged `rail`, through the `op` CLI. Values reach `op` only inside a JSON template on stdin,
//! never as command-line arguments, which other processes can read. Every call runs with a
//! timeout long enough for an unlock prompt.

use std::collections::HashMap;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use super::secrets::{SecretError, SecretStore, TokenSet, json_message};
use crate::config::OnePasswordConfig;
use crate::ops::{OP_REFERENCE_PREFIX, Secret};
use crate::paths::create_private_dir;

/// How long one `op` call may take, unlock prompt included.
pub const OP_TIMEOUT: Duration = Duration::from_secs(30);
const TAG: &str = "rail";
const TOKEN_FIELDS: &str = "label=access_token,label=refresh_token,label=expires_at";
const POLL: Duration = Duration::from_millis(20);
const LOCKED_MARKERS: [&str; 4] = [
    "not currently signed in",
    "not signed in",
    "op signin",
    "authorization prompt dismissed",
];
const NO_ITEM_MARKER: &str = "isn't an item";
const MAX_ERROR_CHARS: usize = 300;
/// `ETXTBSY`: a just-written executable is briefly busy while another thread forks.
const TEXT_FILE_BUSY: i32 = 26;
const SPAWN_ATTEMPTS: usize = 5;
/// The most of stdout or stderr Rail reads from one `op` call.
const MAX_OUTPUT: u64 = 1024 * 1024;

mod process;
use process::classify;
#[cfg(test)]
use process::strip_log_prefix;

/// How to run `op`: which binary, which account, how long to wait.
#[derive(Debug, Clone)]
pub struct OpCli {
    program: PathBuf,
    account: Option<String>,
    timeout: Duration,
    /// Why a configured `op_path` may not be run; every call then fails with it.
    refused: Option<String>,
}

impl OpCli {
    /// `op` on PATH unless `[onepassword] op_path` says otherwise. A configured `op_path` is
    /// checked now (see `check_op_path`); the current user is the owner of `runtime_dir`, which
    /// is created owner-only if it does not exist.
    pub fn from_config(config: Option<&OnePasswordConfig>, runtime_dir: &Path) -> Self {
        let configured = config.and_then(|c| c.op_path.clone());
        let (program, refused) = match configured {
            None => (PathBuf::from("op"), None),
            Some(path) => match own_uid(runtime_dir).and_then(|uid| check_op_path(&path, uid)) {
                // The program that runs is the canonical file that was checked, never the
                // configured spelling, which a symlink swap could point elsewhere.
                Ok(canonical) => (canonical, None),
                Err(reason) => (path, Some(reason)),
            },
        };
        Self {
            program,
            account: config.and_then(|c| c.account.clone()),
            timeout: OP_TIMEOUT,
            refused,
        }
    }

    /// The `--account` Rail passes, if one is configured.
    pub fn account(&self) -> Option<&str> {
        self.account.as_deref()
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Run `op` with `args` (never a secret) and optional `stdin`, and return its stdout.
    /// Every string in `redact` is removed from the error text before it leaves this function.
    pub fn run(
        &self,
        args: &[&str],
        stdin: Option<&[u8]>,
        redact: &[&str],
    ) -> Result<Vec<u8>, SecretError> {
        let mut command = Command::new(&self.program);
        command.args(args);
        if let Some(account) = &self.account {
            command.args(["--account", account.as_str()]);
        }
        let input = if stdin.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        };
        command
            .stdin(input)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let child = self.spawn(&mut command)?;
        let (status, stdout, stderr) = process::run_child(child, stdin, self.timeout)?;
        if status.success() {
            return Ok(stdout);
        }
        Err(classify(&stderr, redact))
    }

    fn spawn(&self, command: &mut Command) -> Result<Child, SecretError> {
        if let Some(reason) = &self.refused {
            return Err(SecretError::OnePasswordUnsafe {
                path: self.program.display().to_string(),
                reason: reason.clone(),
            });
        }
        let mut attempt = 1;
        loop {
            match command.spawn() {
                Err(e) if e.raw_os_error() == Some(TEXT_FILE_BUSY) && attempt < SPAWN_ATTEMPTS => {
                    attempt += 1;
                    std::thread::sleep(POLL);
                }
                result => {
                    return result.map_err(|e| SecretError::OnePasswordMissing {
                        program: self.program.display().to_string(),
                        message: e.to_string(),
                    });
                }
            }
        }
    }

    /// Whether `op` is installed, unlocked and signed in.
    pub fn whoami(&self) -> Result<(), SecretError> {
        self.run(&["whoami", "--format", "json"], None, &[])
            .map(drop)
    }

    /// The value behind an `op://vault/item/field` reference. Never cached or logged.
    pub fn read_reference(&self, reference: &str) -> Result<Secret, SecretError> {
        validate_reference(reference)?;
        let out = self.run(&["read", "--no-newline", reference], None, &[])?;
        let text = String::from_utf8(out)
            .map_err(|_| SecretError::OnePasswordInvalid(format!("{reference} is not text")))?;
        Ok(Secret::new(text.trim_end_matches(['\r', '\n'])))
    }
}

/// The current user's uid, read without `unsafe` as the owner of a directory Rail creates.
fn own_uid(runtime_dir: &Path) -> Result<u32, String> {
    create_private_dir(runtime_dir)
        .and_then(|()| std::fs::metadata(runtime_dir))
        .map(|m| m.uid())
        .map_err(|e| format!("cannot tell who the current user is ({e})"))
}

/// A configured `op` binary must resolve (symlinks followed once, here) to a regular,
/// executable file owned by root or by `own_uid` and writable by nobody else, inside
/// directories that only root or `own_uid` own and that others cannot write to (unless sticky,
/// as `/tmp` is). Anything less would let another user swap the program and read the tokens
/// on its stdin. Returns the canonical path, which is the one to run.
pub fn check_op_path(path: &Path, own_uid: u32) -> Result<PathBuf, String> {
    if !path.is_absolute() {
        return Err("op_path must be an absolute path".to_string());
    }
    let canonical =
        std::fs::canonicalize(path).map_err(|e| format!("it cannot be resolved ({e})"))?;
    let meta = std::fs::metadata(&canonical).map_err(|e| format!("it cannot be read ({e})"))?;
    if !meta.is_file() {
        return Err("it is not a regular file".to_string());
    }
    let mode = meta.permissions().mode();
    if mode & 0o111 == 0 {
        return Err("it is not executable".to_string());
    }
    if mode & 0o022 != 0 {
        return Err("it is writable by group or others".to_string());
    }
    check_owner(&meta, own_uid, "it")?;
    for dir in canonical.ancestors().skip(1) {
        check_directory(dir, own_uid)?;
    }
    Ok(canonical)
}

fn check_owner(meta: &std::fs::Metadata, own_uid: u32, what: &str) -> Result<(), String> {
    if meta.uid() != 0 && meta.uid() != own_uid {
        return Err(format!("{what} is owned by another user"));
    }
    Ok(())
}

/// A directory on the way to `op`: owned by root or the user, and not writable by group or
/// others unless the sticky bit stops them replacing entries they do not own.
fn check_directory(dir: &Path, own_uid: u32) -> Result<(), String> {
    let meta = std::fs::metadata(dir)
        .map_err(|e| format!("the directory {} cannot be read ({e})", dir.display()))?;
    check_owner(&meta, own_uid, &format!("the directory {}", dir.display()))?;
    let mode = meta.permissions().mode();
    if mode & 0o022 != 0 && mode & 0o1000 == 0 {
        return Err(format!(
            "the directory {} is writable by group or others",
            dir.display()
        ));
    }
    Ok(())
}

/// `op://vault/item/field`, every part present.
pub fn validate_reference(reference: &str) -> Result<(), SecretError> {
    let parts: Vec<&str> = reference
        .strip_prefix(OP_REFERENCE_PREFIX)
        .map(|rest| rest.split('/').collect())
        .unwrap_or_default();
    if parts.len() < 3 || parts.iter().any(|p| p.trim().is_empty()) {
        // Never echo the value: it may be a secret pasted where a reference belongs.
        return Err(SecretError::OnePasswordInvalid(
            "the client secret reference is not of the form op://vault/item/field".to_string(),
        ));
    }
    Ok(())
}

fn parse_json<T: DeserializeOwned>(out: &[u8]) -> Result<T, SecretError> {
    serde_json::from_slice(out).map_err(|e| SecretError::Encoding(json_message(&e)))
}

#[derive(Deserialize)]
struct ItemRef {
    id: String,
    #[serde(default)]
    title: String,
}

#[derive(Deserialize)]
struct FieldValue {
    label: String,
    #[serde(default)]
    value: String,
}

/// An item template. `fields` is last, so the JSON ends with the field array.
#[derive(Serialize)]
struct ItemTemplate<'a> {
    title: &'a str,
    category: &'static str,
    tags: [&'static str; 1],
    fields: Vec<TemplateField>,
}

#[derive(Serialize)]
struct TemplateField {
    id: &'static str,
    label: &'static str,
    #[serde(rename = "type")]
    kind: &'static str,
    value: String,
}

fn concealed(id: &'static str, value: &str) -> TemplateField {
    TemplateField {
        id,
        label: id,
        kind: "CONCEALED",
        value: value.to_string(),
    }
}

fn template_json(template: &ItemTemplate<'_>) -> Result<Vec<u8>, SecretError> {
    serde_json::to_vec(template).map_err(|e| SecretError::Encoding(json_message(&e)))
}

fn token_template<'a>(title: &'a str, tokens: &TokenSet) -> ItemTemplate<'a> {
    ItemTemplate {
        title,
        category: "SECURE_NOTE",
        tags: [TAG],
        fields: vec![
            concealed("access_token", &tokens.access_token),
            concealed("refresh_token", &tokens.refresh_token),
            TemplateField {
                id: "expires_at",
                label: "expires_at",
                kind: "STRING",
                value: tokens.expires_at.to_string(),
            },
        ],
    }
}

fn password_template<'a>(title: &'a str, password: &Secret<String>) -> ItemTemplate<'a> {
    ItemTemplate {
        title,
        category: "SECURE_NOTE",
        tags: [TAG],
        fields: vec![concealed("password", password.expose())],
    }
}

fn tokens_from_fields(fields: &[FieldValue]) -> Result<TokenSet, SecretError> {
    let field = |label: &str| {
        fields
            .iter()
            .find(|f| f.label == label)
            .map(|f| f.value.clone())
            .ok_or_else(|| SecretError::Encoding(format!("the 1Password item has no {label}")))
    };
    let expires_at = field("expires_at")?
        .trim()
        .parse()
        .map_err(|_| SecretError::Encoding("expires_at is not a number".to_string()))?;
    Ok(TokenSet {
        access_token: field("access_token")?,
        refresh_token: field("refresh_token")?,
        expires_at,
    })
}

/// Item ids by vault and title, so most calls skip the `op item list` lookup.
#[derive(Debug, Clone, Default)]
pub struct ItemIdCache(Arc<Mutex<HashMap<String, String>>>);

impl ItemIdCache {
    fn key(vault: &str, title: &str) -> String {
        format!("{vault}/{title}")
    }

    fn get(&self, vault: &str, title: &str) -> Option<String> {
        let ids = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        ids.get(&Self::key(vault, title)).cloned()
    }

    fn put(&self, vault: &str, title: &str, id: &str) {
        let mut ids = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        ids.insert(Self::key(vault, title), id.to_string());
    }

    fn forget(&self, vault: &str, title: &str) {
        let mut ids = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        ids.remove(&Self::key(vault, title));
    }
}

pub struct OnePasswordSecretStore {
    op: OpCli,
    vault: String,
    ids: ItemIdCache,
}

fn item_title(address: &str) -> String {
    format!("Rail: {}", address.to_ascii_lowercase())
}

impl OnePasswordSecretStore {
    pub fn new(op: OpCli, vault: String, ids: ItemIdCache) -> Self {
        Self { op, vault, ids }
    }

    /// The cache scope of a vault: the account too, since two accounts can use one vault name.
    fn scope(&self, vault: &str) -> String {
        format!("{}\u{0}{vault}", self.op.account().unwrap_or(""))
    }

    fn vault(&self) -> Result<&str, SecretError> {
        if self.vault.trim().is_empty() {
            return Err(SecretError::OnePasswordInvalid(
                "no vault is set; add vault = \"...\" under [onepassword]".to_string(),
            ));
        }
        Ok(&self.vault)
    }

    fn find(&self, title: &str) -> Result<Option<String>, SecretError> {
        let vault = self.vault()?;
        if let Some(id) = self.ids.get(&self.scope(vault), title) {
            return Ok(Some(id));
        }
        self.look_up(title)
    }

    /// Ask `op item list` for the item (never `--reveal`, never the cache), and remember its id.
    fn look_up(&self, title: &str) -> Result<Option<String>, SecretError> {
        let vault = self.vault()?;
        let args = [
            "item", "list", "--vault", vault, "--tags", TAG, "--format", "json",
        ];
        let out = self.op.run(&args, None, &[])?;
        let items: Vec<ItemRef> = if out.iter().all(u8::is_ascii_whitespace) {
            Vec::new()
        } else {
            parse_json(&out)?
        };
        let id = items.into_iter().find(|i| i.title == title).map(|i| i.id);
        match &id {
            Some(id) => self.ids.put(&self.scope(vault), title, id),
            None => self.ids.forget(&self.scope(vault), title),
        }
        Ok(id)
    }

    /// The item's tokens, revealed. `OnePasswordNoItem` when the id is gone.
    fn read_item(&self, id: &str, vault: &str) -> Result<TokenSet, SecretError> {
        let args = [
            "item",
            "get",
            id,
            "--vault",
            vault,
            "--fields",
            TOKEN_FIELDS,
            "--reveal",
            "--format",
            "json",
        ];
        let out = self.op.run(&args, None, &[])?;
        tokens_from_fields(&parse_json::<Vec<FieldValue>>(&out)?)
    }

    /// Read an edited item back: `op item edit` takes its template on stdin, and a CLI that
    /// ignored it would answer success and keep the old tokens. Anything but the values just
    /// written (a missing item included) is an error, so nothing is reported as saved.
    fn verify_edit(&self, id: &str, vault: &str, written: &TokenSet) -> Result<(), SecretError> {
        match self.read_item(id, vault) {
            Ok(stored) if stored == *written => Ok(()),
            Ok(_) | Err(SecretError::OnePasswordNoItem(_)) => {
                Err(SecretError::OnePasswordEditUnverified)
            }
            Err(e) => Err(e),
        }
    }

    fn read_password_item(
        &self,
        id: &str,
        vault: &str,
    ) -> Result<Option<Secret<String>>, SecretError> {
        let args = [
            "item",
            "get",
            id,
            "--vault",
            vault,
            "--fields",
            "label=password",
            "--reveal",
            "--format",
            "json",
        ];
        let out = self.op.run(&args, None, &[])?;
        let fields: Vec<FieldValue> = parse_json(&out)?;
        let val = fields
            .into_iter()
            .find(|f| f.label == "password")
            .map(|f| Secret::new(f.value));
        Ok(val)
    }

    fn verify_password_edit(
        &self,
        id: &str,
        vault: &str,
        written: &Secret<String>,
    ) -> Result<(), SecretError> {
        match self.read_password_item(id, vault) {
            Ok(Some(stored)) if stored == *written => Ok(()),
            Ok(_) | Err(SecretError::OnePasswordNoItem(_)) => {
                Err(SecretError::OnePasswordEditUnverified)
            }
            Err(e) => Err(e),
        }
    }

    fn create(&self, title: &str, json: &[u8], redact: &[&str]) -> Result<(), SecretError> {
        let vault = self.vault()?;
        let args = ["item", "create", "--vault", vault, "--format", "json", "-"];
        let created: ItemRef = parse_json(&self.op.run(&args, Some(json), redact)?)?;
        self.ids.put(&self.scope(vault), title, &created.id);
        Ok(())
    }
}

impl SecretStore for OnePasswordSecretStore {
    fn get_tokens(&self, address: &str) -> Result<Option<TokenSet>, SecretError> {
        let title = item_title(address);
        let Some(id) = self.find(&title)? else {
            return Ok(None);
        };
        let vault = self.vault()?;
        match self.read_item(&id, vault) {
            Err(SecretError::OnePasswordNoItem(_)) => {
                self.ids.forget(&self.scope(vault), &title);
                Ok(None)
            }
            read => read.map(Some),
        }
    }

    fn set_tokens(&self, address: &str, tokens: &TokenSet) -> Result<(), SecretError> {
        let title = item_title(address);
        let json = template_json(&token_template(&title, tokens))?;
        let redact = [tokens.access_token.as_str(), tokens.refresh_token.as_str()];
        let Some(id) = self.find(&title)? else {
            return self.create(&title, &json, &redact);
        };
        let vault = self.vault()?;
        let args = ["item", "edit", &id, "--vault", vault, "--format", "json"];
        match self.op.run(&args, Some(&json), &redact) {
            Ok(_) => self.verify_edit(&id, vault, tokens),
            Err(SecretError::OnePasswordNoItem(_)) => {
                self.ids.forget(&self.scope(vault), &title);
                self.create(&title, &json, &redact)
            }
            Err(e) => Err(e),
        }
    }

    fn delete_tokens(&self, address: &str) -> Result<(), SecretError> {
        let title = item_title(address);
        let Some(id) = self.find(&title)? else {
            return Ok(());
        };
        let vault = self.vault()?;
        match self
            .op
            .run(&["item", "delete", &id, "--vault", vault], None, &[])
        {
            Ok(_) | Err(SecretError::OnePasswordNoItem(_)) => {
                self.ids.forget(&self.scope(vault), &title);
                Ok(())
            }
            Err(e) => Err(e),
        }
    }

    fn get_password(&self, key: &str) -> Result<Option<Secret<String>>, SecretError> {
        let title = item_title(key);
        let Some(id) = self.find(&title)? else {
            return Ok(None);
        };
        let vault = self.vault()?;
        match self.read_password_item(&id, vault) {
            Err(SecretError::OnePasswordNoItem(_)) => {
                self.ids.forget(&self.scope(vault), &title);
                Ok(None)
            }
            Ok(opt) => Ok(opt),
            Err(e) => Err(e),
        }
    }

    fn set_password(&self, key: &str, password: &Secret<String>) -> Result<(), SecretError> {
        let title = item_title(key);
        let json = template_json(&password_template(&title, password))?;
        let redact = [password.expose()];
        let Some(id) = self.find(&title)? else {
            return self.create(&title, &json, &redact);
        };
        let vault = self.vault()?;
        let args = ["item", "edit", &id, "--vault", vault, "--format", "json"];
        match self.op.run(&args, Some(&json), &redact) {
            Ok(_) => self.verify_password_edit(&id, vault, password),
            Err(SecretError::OnePasswordNoItem(_)) => {
                self.ids.forget(&self.scope(vault), &title);
                self.create(&title, &json, &redact)
            }
            Err(e) => Err(e),
        }
    }

    fn delete_password(&self, key: &str) -> Result<(), SecretError> {
        let title = item_title(key);
        let Some(id) = self.find(&title)? else {
            return Ok(());
        };
        let vault = self.vault()?;
        match self
            .op
            .run(&["item", "delete", &id, "--vault", vault], None, &[])
        {
            Ok(_) | Err(SecretError::OnePasswordNoItem(_)) => {
                self.ids.forget(&self.scope(vault), &title);
                Ok(())
            }
            Err(e) => Err(e),
        }
    }

    fn has_password(&self, key: &str) -> Result<bool, SecretError> {
        self.look_up(&item_title(key)).map(|id| id.is_some())
    }

    fn backend_name(&self) -> &'static str {
        "1password"
    }

    /// The item's id by an uncached lookup (no `--reveal`).
    fn item_id(&self, address: &str) -> Result<Option<String>, SecretError> {
        self.look_up(&item_title(address))
    }

    /// An item lookup by tag and title: no token is fetched, so polling stays cheap and
    /// loads nothing secret into memory.
    fn has_tokens(&self, address: &str) -> Result<bool, SecretError> {
        self.look_up(&item_title(address)).map(|id| id.is_some())
    }

    fn probe(&self) -> Result<(), SecretError> {
        self.vault()?;
        self.op.whoami()
    }
}

/// Put a client secret in 1Password and return the `op://` reference to write into config.
pub fn store_client_secret(
    op: &OpCli,
    vault: &str,
    client_id: &str,
    secret: &Secret,
) -> Result<String, SecretError> {
    let title = format!("Rail client: {client_id}");
    let template = ItemTemplate {
        title: &title,
        category: "SECURE_NOTE",
        tags: [TAG],
        fields: vec![concealed("client_secret", secret.expose())],
    };
    let args = ["item", "create", "--vault", vault, "--format", "json", "-"];
    let out = op.run(&args, Some(&template_json(&template)?), &[secret.expose()])?;
    let created: ItemRef = parse_json(&out)?;
    Ok(format!(
        "{OP_REFERENCE_PREFIX}{vault}/{}/client_secret",
        created.id
    ))
}

#[cfg(test)]
mod tests;
