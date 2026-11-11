//! Hands out a valid access token, refreshing it when needed. One refresh at a time per account.
//!
//! A refresh is written back to the secret store only when the store would otherwise lose
//! something it needs: a new refresh token. The access token lives an hour and is kept in
//! memory; after a restart Rail simply refreshes again. Each write to 1Password can mean an
//! authorisation prompt, so a refresh token the provider rotates on every refresh (Microsoft
//! does; the old one stays valid for weeks) is saved at most once per `ROTATED_SAVE_SECS`.
//!
//! The tokens in memory outlive failures. A refresh that fails for a passing reason (the
//! network, the provider) keeps them, and a save that fails (1Password locked, a prompt nobody
//! answered) is retried at the next refresh while the new tokens stay in use. Either way the
//! next sync needs no read from the store, so an unattended daemon keeps syncing. Only a
//! refusal that needs a new login drops them, so the tokens that login stores are read afresh.

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use oauth2::RefreshToken;
use tokio::sync::Mutex;

use super::AuthError;
use super::client::{OAuthClient, send_token_request};
use super::flow::{map_token_error, token_set_from_response};
use super::secrets::{SecretStore, TokenSet};

/// Refresh when fewer than this many seconds remain.
const REFRESH_MARGIN_SECS: i64 = 60;

/// The longest a rotated refresh token goes unsaved. A day keeps the stored one far inside
/// Microsoft's 90-day lifetime while sparing the store (and 1Password's prompts) 23 writes.
const ROTATED_SAVE_SECS: i64 = 24 * 60 * 60;

/// Caches an account's tokens and refreshes them on demand, one refresh at a time.
pub struct TokenSource {
    address: String,
    client: OAuthClient,
    http: reqwest::Client,
    secrets: Arc<dyn SecretStore>,
    cached: Mutex<Cache>,
    clock: Box<dyn Fn() -> i64 + Send + Sync>,
}

/// What the source holds: the tokens in use, and the refresh token the store has.
#[derive(Default)]
struct Cache {
    tokens: Option<TokenSet>,
    stored: Option<Stored>,
}

/// The refresh token the secret store holds, and when this source last saw it there.
struct Stored {
    refresh_token: String,
    at: i64,
}

/// Whether a refresh that produced `refresh_token` at `now` must be written to the store.
fn must_save(stored: Option<&Stored>, refresh_token: &str, now: i64) -> bool {
    stored.is_none_or(|s| s.refresh_token != refresh_token && now - s.at >= ROTATED_SAVE_SECS)
}

/// Current Unix time in seconds.
pub fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
}

impl TokenSource {
    /// A token source for the account with this address, backed by `secrets`.
    pub fn new(
        address: impl Into<String>,
        client: OAuthClient,
        http: reqwest::Client,
        secrets: Arc<dyn SecretStore>,
    ) -> Self {
        Self {
            address: address.into(),
            client,
            http,
            secrets,
            cached: Mutex::new(Cache::default()),
            clock: Box::new(unix_now),
        }
    }

    /// Replace the clock (Unix seconds); for tests.
    pub fn with_clock(mut self, clock: impl Fn() -> i64 + Send + Sync + 'static) -> Self {
        self.clock = Box::new(clock);
        self
    }

    /// The account address this source serves.
    pub fn address(&self) -> &str {
        &self.address
    }

    /// A bearer token valid for at least `REFRESH_MARGIN_SECS`.
    pub async fn access_token(&self) -> Result<String, AuthError> {
        // Holding the lock across the refresh is the single-flight guarantee.
        let mut cache = self.cached.lock().await;
        let now = (self.clock)();
        if let Some(tokens) = cache.tokens.as_ref().filter(|t| fresh(t, now)) {
            return Ok(tokens.access_token.clone());
        }
        let current = self.current(&mut cache, now)?;
        let tokens = if fresh(&current, now) {
            current
        } else {
            self.refresh(&mut cache, &current, now).await?
        };
        let access = tokens.access_token.clone();
        cache.tokens = Some(tokens);
        Ok(access)
    }

    /// A new access token whatever the cached one's expiry says: the API refused it (HTTP 401).
    pub async fn force_refresh(&self) -> Result<String, AuthError> {
        let mut cache = self.cached.lock().await;
        let now = (self.clock)();
        let current = self.current(&mut cache, now)?;
        let tokens = self.refresh(&mut cache, &current, now).await?;
        let access = tokens.access_token.clone();
        cache.tokens = Some(tokens);
        Ok(access)
    }

    /// The tokens in use, read from the store the first time (which records what it holds).
    /// They stay in `cache` until a refresh replaces them or needs a new login.
    fn current(&self, cache: &mut Cache, now: i64) -> Result<TokenSet, AuthError> {
        if let Some(tokens) = &cache.tokens {
            return Ok(tokens.clone());
        }
        let tokens = self
            .secrets
            .get_tokens(&self.address)?
            .ok_or_else(|| AuthError::NeedsAuth("no stored tokens".to_string()))?;
        cache.stored = Some(Stored {
            refresh_token: tokens.refresh_token.clone(),
            at: now,
        });
        cache.tokens = Some(tokens.clone());
        Ok(tokens)
    }

    /// New tokens for `current`. A refusal that needs a new login forgets the tokens in
    /// memory; any other failure keeps them for the next try.
    async fn refresh(
        &self,
        cache: &mut Cache,
        current: &TokenSet,
        now: i64,
    ) -> Result<TokenSet, AuthError> {
        let result = self.exchange(current, now).await;
        if let Err(AuthError::NeedsAuth(_)) = &result {
            cache.tokens = None;
        }
        let tokens = result?;
        if must_save(cache.stored.as_ref(), &tokens.refresh_token, now) {
            self.save(cache, &tokens, now);
        }
        Ok(tokens)
    }

    /// Write `tokens` to the store. A failure is logged, not returned: the stored refresh
    /// token still works, the new tokens stay in use, and the next refresh tries again.
    fn save(&self, cache: &mut Cache, tokens: &TokenSet, now: i64) {
        match self.secrets.set_tokens(&self.address, tokens) {
            Ok(()) => {
                cache.stored = Some(Stored {
                    refresh_token: tokens.refresh_token.clone(),
                    at: now,
                });
            }
            Err(e) => tracing::warn!(
                address = %self.address,
                error = %e,
                "could not save refreshed tokens; keeping them in memory and trying at the next refresh"
            ),
        }
    }

    async fn exchange(&self, current: &TokenSet, now: i64) -> Result<TokenSet, AuthError> {
        tracing::debug!(address = %self.address, "refreshing access token");
        let response = self
            .client
            .exchange_refresh_token(&RefreshToken::new(current.refresh_token.clone()))
            .request_async(&|req| send_token_request(self.http.clone(), req))
            .await
            .map_err(map_token_error)?;
        token_set_from_response(&response, Some(&current.refresh_token), now)
    }
}

fn fresh(tokens: &TokenSet, now: i64) -> bool {
    tokens.expires_at - now > REFRESH_MARGIN_SECS
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::client::{build_client, http_client};
    use crate::auth::endpoints::endpoints_for;
    use crate::auth::secrets::{MemorySecretStore, SecretError};
    use crate::config::{AccountConfig, AccountKind};
    use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
    use wiremock::matchers::{body_string_contains, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    const ADDRESS: &str = "me@example.com";

    fn source(server_uri: &str, secrets: Arc<dyn SecretStore>, now: i64) -> TokenSource {
        let account = AccountConfig {
            name: "w".into(),
            address: ADDRESS.into(),
            color: None,
            label: None,
            ..crate::test_support::sample_account()
        };
        let mut endpoints = endpoints_for(AccountKind::Gmail, None);
        endpoints.token_url = format!("{server_uri}/token");
        let client = build_client(&account, &endpoints, None).unwrap();
        TokenSource::new(ADDRESS, client, http_client().unwrap(), secrets).with_clock(move || now)
    }

    fn stored(expires_at: i64) -> Arc<dyn SecretStore> {
        let s = MemorySecretStore::default();
        s.set_tokens(
            ADDRESS,
            &TokenSet {
                access_token: "old-access".into(),
                refresh_token: "old-refresh".into(),
                expires_at,
            },
        )
        .unwrap();
        Arc::new(s)
    }

    #[tokio::test]
    async fn fresh_token_is_returned_without_a_request() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(500))
            .expect(0)
            .mount(&server)
            .await;
        let src = source(&server.uri(), stored(10_000), 1_000);
        assert_eq!(src.access_token().await.unwrap(), "old-access");
        assert_eq!(src.access_token().await.unwrap(), "old-access");
    }

    /// A 401 from the API means the access token is bad however fresh it looks.
    #[tokio::test]
    async fn force_refresh_refreshes_even_a_fresh_token() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "access_token": "forced", "expires_in": 3600, "token_type": "Bearer"
            })))
            .expect(1)
            .mount(&server)
            .await;
        let src = source(&server.uri(), stored(10_000), 1_000);
        assert_eq!(src.access_token().await.unwrap(), "old-access");
        assert_eq!(src.force_refresh().await.unwrap(), "forced");
        assert_eq!(src.access_token().await.unwrap(), "forced");
    }

    #[tokio::test]
    async fn token_inside_refresh_margin_is_refreshed() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/token"))
            .and(body_string_contains("grant_type=refresh_token"))
            .and(body_string_contains("refresh_token=old-refresh"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "access_token": "new-access", "refresh_token": "new-refresh", "expires_in": 3600, "token_type": "Bearer"
            })))
            .expect(1)
            .mount(&server)
            .await;
        let secrets = stored(1_030); // expires in 30s, margin is 60s
        let src = source(&server.uri(), secrets.clone(), 1_000);
        assert_eq!(src.access_token().await.unwrap(), "new-access");
        // The rotated token is not due for saving yet: the stored one still works.
        let saved = secrets.get_tokens(ADDRESS).unwrap().unwrap();
        assert_eq!(saved.refresh_token, "old-refresh");
    }

    /// Counts the writes that reach the store; each one may be a 1Password prompt.
    #[derive(Default)]
    /// Counts reads and writes (each may be a 1Password prompt); `locked` makes both fail
    /// as a locked 1Password or an unanswered prompt does.
    struct CountingStore {
        inner: MemorySecretStore,
        reads: std::sync::atomic::AtomicUsize,
        writes: std::sync::atomic::AtomicUsize,
        locked: AtomicBool,
    }

    impl CountingStore {
        fn check_lock(&self) -> Result<(), SecretError> {
            if self.locked.load(Ordering::SeqCst) {
                return Err(SecretError::OnePasswordLocked("locked".into()));
            }
            Ok(())
        }
    }

    impl SecretStore for CountingStore {
        fn get_tokens(&self, address: &str) -> Result<Option<TokenSet>, SecretError> {
            self.reads.fetch_add(1, Ordering::SeqCst);
            self.check_lock()?;
            self.inner.get_tokens(address)
        }
        fn set_tokens(&self, address: &str, tokens: &TokenSet) -> Result<(), SecretError> {
            self.writes.fetch_add(1, Ordering::SeqCst);
            self.check_lock()?;
            self.inner.set_tokens(address, tokens)
        }
        fn delete_tokens(&self, address: &str) -> Result<(), SecretError> {
            self.inner.delete_tokens(address)
        }
        fn backend_name(&self) -> &'static str {
            "counting"
        }
    }

    fn counting(expires_at: i64) -> Arc<CountingStore> {
        let store = CountingStore::default();
        store
            .inner
            .set_tokens(
                ADDRESS,
                &TokenSet {
                    access_token: "old-access".into(),
                    refresh_token: "old-refresh".into(),
                    expires_at,
                },
            )
            .unwrap();
        Arc::new(store)
    }

    /// A source whose clock the test moves.
    fn ticking(
        server_uri: &str,
        secrets: Arc<dyn SecretStore>,
        clock: &Arc<AtomicI64>,
    ) -> TokenSource {
        let clock = clock.clone();
        source(server_uri, secrets, 0).with_clock(move || clock.load(Ordering::SeqCst))
    }

    async fn token_server(body: serde_json::Value) -> MockServer {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(body))
            .mount(&server)
            .await;
        server
    }

    /// Google keeps the refresh token: a day of hourly refreshes never writes to the store.
    #[tokio::test]
    async fn unchanged_refresh_token_is_never_written_back() {
        let server = token_server(serde_json::json!({
            "access_token": "new-access", "expires_in": 3600, "token_type": "Bearer"
        }))
        .await;
        let store = counting(0);
        let clock = Arc::new(AtomicI64::new(1_000));
        let src = ticking(&server.uri(), store.clone(), &clock);
        for hour in 0..30 {
            clock.store(1_000 + hour * 3_600, Ordering::SeqCst);
            assert_eq!(src.access_token().await.unwrap(), "new-access");
        }
        assert_eq!(store.writes.load(Ordering::SeqCst), 0);
        assert_eq!(server.received_requests().await.unwrap().len(), 30);
    }

    /// Microsoft rotates the refresh token on every refresh: it is saved once a day.
    #[tokio::test]
    async fn rotated_refresh_token_is_saved_once_a_day() {
        let server = token_server(serde_json::json!({
            "access_token": "new-access", "refresh_token": "rotated",
            "expires_in": 3600, "token_type": "Bearer"
        }))
        .await;
        let store = counting(0);
        let clock = Arc::new(AtomicI64::new(1_000));
        let src = ticking(&server.uri(), store.clone(), &clock);
        for hour in 0..24 {
            clock.store(1_000 + hour * 3_600, Ordering::SeqCst);
            src.access_token().await.unwrap();
        }
        assert_eq!(
            store.writes.load(Ordering::SeqCst),
            0,
            "within the first day"
        );
        clock.store(1_000 + ROTATED_SAVE_SECS, Ordering::SeqCst);
        src.force_refresh().await.unwrap();
        assert_eq!(store.writes.load(Ordering::SeqCst), 1);
        let saved = store.get_tokens(ADDRESS).unwrap().unwrap();
        assert_eq!(saved.refresh_token, "rotated");
        assert_eq!(saved.access_token, "new-access");
    }

    /// A refresh that fails for a passing reason keeps the tokens: the retry reads nothing.
    #[tokio::test]
    async fn failed_refresh_keeps_tokens_in_memory() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(503))
            .up_to_n_times(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "access_token": "new-access", "expires_in": 3600, "token_type": "Bearer"
            })))
            .mount(&server)
            .await;
        let store = counting(0);
        let src = source(&server.uri(), store.clone(), 1_000);
        assert!(matches!(
            src.access_token().await,
            Err(AuthError::Network(_))
        ));
        store.locked.store(true, Ordering::SeqCst);
        assert_eq!(src.access_token().await.unwrap(), "new-access");
        assert_eq!(
            store.reads.load(Ordering::SeqCst),
            1,
            "read once, at the start"
        );
    }

    /// A save that fails does not fail the refresh, and is tried again at the next refresh.
    #[tokio::test]
    async fn failed_save_keeps_new_tokens_and_retries() {
        let server = token_server(serde_json::json!({
            "access_token": "new-access", "refresh_token": "rotated",
            "expires_in": 3600, "token_type": "Bearer"
        }))
        .await;
        let store = counting(0);
        let clock = Arc::new(AtomicI64::new(1_000));
        let src = ticking(&server.uri(), store.clone(), &clock);
        src.access_token().await.unwrap();
        store.locked.store(true, Ordering::SeqCst);
        clock.store(1_000 + ROTATED_SAVE_SECS, Ordering::SeqCst);
        assert_eq!(src.force_refresh().await.unwrap(), "new-access");
        assert_eq!(store.writes.load(Ordering::SeqCst), 1, "tried once");
        clock.store(1_000 + ROTATED_SAVE_SECS + 30, Ordering::SeqCst);
        assert_eq!(
            src.access_token().await.unwrap(),
            "new-access",
            "from memory"
        );
        assert_eq!(store.reads.load(Ordering::SeqCst), 1);
        store.locked.store(false, Ordering::SeqCst);
        clock.store(1_000 + ROTATED_SAVE_SECS + 3_600, Ordering::SeqCst);
        src.access_token().await.unwrap();
        assert_eq!(store.writes.load(Ordering::SeqCst), 2, "tried again");
        let saved = store.get_tokens(ADDRESS).unwrap().unwrap();
        assert_eq!(saved.refresh_token, "rotated");
    }

    /// A refusal that needs a new login forgets the tokens, so the login's are read afresh.
    #[tokio::test]
    async fn needs_auth_rereads_the_store() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(400).set_body_json(serde_json::json!({
                "error": "invalid_grant"
            })))
            .mount(&server)
            .await;
        let store = counting(0);
        let src = source(&server.uri(), store.clone(), 1_000);
        assert!(matches!(
            src.access_token().await,
            Err(AuthError::NeedsAuth(_))
        ));
        let fresh_login = TokenSet {
            access_token: "logged-in".into(),
            refresh_token: "new-login".into(),
            expires_at: 10_000,
        };
        store.inner.set_tokens(ADDRESS, &fresh_login).unwrap();
        assert_eq!(src.access_token().await.unwrap(), "logged-in");
        assert_eq!(store.reads.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn must_save_rules() {
        let stored = Stored {
            refresh_token: "a".into(),
            at: 100,
        };
        assert!(must_save(None, "a", 100), "nothing known about the store");
        assert!(!must_save(Some(&stored), "a", 100 + 10 * ROTATED_SAVE_SECS));
        assert!(!must_save(Some(&stored), "b", 100 + ROTATED_SAVE_SECS - 1));
        assert!(must_save(Some(&stored), "b", 100 + ROTATED_SAVE_SECS));
    }

    #[tokio::test]
    async fn refresh_keeps_old_refresh_token_when_omitted() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "access_token": "new-access", "expires_in": 3600, "token_type": "Bearer"
            })))
            .mount(&server)
            .await;
        let secrets = stored(0);
        let src = source(&server.uri(), secrets.clone(), 1_000);
        assert_eq!(src.access_token().await.unwrap(), "new-access");
        assert_eq!(
            secrets.get_tokens(ADDRESS).unwrap().unwrap().refresh_token,
            "old-refresh"
        );
    }

    #[tokio::test]
    async fn invalid_grant_means_needs_auth() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(400).set_body_json(serde_json::json!({
                "error": "invalid_grant", "error_description": "Token has been expired or revoked."
            })))
            .mount(&server)
            .await;
        let src = source(&server.uri(), stored(0), 1_000);
        let err = src.access_token().await.unwrap_err();
        assert!(
            matches!(err, AuthError::NeedsAuth(ref m) if m.contains("revoked")),
            "{err:?}"
        );
        assert_eq!(err.class(), crate::error::ErrorClass::Permanent);
    }

    async fn refresh_error(response: ResponseTemplate) -> AuthError {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(response)
            .mount(&server)
            .await;
        let src = source(&server.uri(), stored(0), 1_000);
        src.access_token().await.unwrap_err()
    }

    #[tokio::test]
    async fn token_endpoint_503_with_html_body_is_transient() {
        let err = refresh_error(
            ResponseTemplate::new(503)
                .set_body_raw("<html>Service Unavailable</html>", "text/html"),
        )
        .await;
        assert!(
            matches!(err, AuthError::Network(ref m) if m.contains("503")),
            "{err:?}"
        );
        assert_eq!(err.class(), crate::error::ErrorClass::Transient);
    }

    #[tokio::test]
    async fn token_endpoint_503_with_empty_body_is_transient() {
        let err = refresh_error(ResponseTemplate::new(503)).await;
        assert!(
            matches!(err, AuthError::Network(ref m) if m.contains("503")),
            "{err:?}"
        );
        assert_eq!(err.class(), crate::error::ErrorClass::Transient);
    }

    #[tokio::test]
    async fn token_endpoint_429_is_transient() {
        let err = refresh_error(ResponseTemplate::new(429).set_body_json(serde_json::json!({
            "error": "rate_limited"
        })))
        .await;
        assert!(
            matches!(err, AuthError::Network(ref m) if m.contains("429")),
            "{err:?}"
        );
        assert_eq!(err.class(), crate::error::ErrorClass::Transient);
    }

    #[tokio::test]
    async fn temporarily_unavailable_error_response_is_transient() {
        for code in ["temporarily_unavailable", "server_error"] {
            let err = refresh_error(
                ResponseTemplate::new(400).set_body_json(serde_json::json!({ "error": code })),
            )
            .await;
            assert_eq!(err.class(), crate::error::ErrorClass::Transient, "{err:?}");
        }
    }

    #[tokio::test]
    async fn other_4xx_responses_stay_permanent() {
        let err = refresh_error(
            ResponseTemplate::new(401)
                .set_body_json(serde_json::json!({ "error": "invalid_client" })),
        )
        .await;
        assert!(matches!(err, AuthError::Exchange(_)), "{err:?}");
        assert_eq!(err.class(), crate::error::ErrorClass::Permanent);
        let err =
            refresh_error(ResponseTemplate::new(404).set_body_raw("<html/>", "text/html")).await;
        assert_eq!(err.class(), crate::error::ErrorClass::Permanent, "{err:?}");
    }

    #[tokio::test]
    async fn unreachable_endpoint_is_transient() {
        let src = source("http://127.0.0.1:9", stored(0), 1_000);
        let err = src.access_token().await.unwrap_err();
        assert!(matches!(err, AuthError::Network(_)), "{err:?}");
        assert_eq!(err.class(), crate::error::ErrorClass::Transient);
    }

    #[tokio::test]
    async fn no_stored_tokens_means_needs_auth() {
        let server = MockServer::start().await;
        let src = source(&server.uri(), Arc::new(MemorySecretStore::default()), 1_000);
        assert!(matches!(
            src.access_token().await.unwrap_err(),
            AuthError::NeedsAuth(_)
        ));
    }

    #[tokio::test]
    async fn concurrent_callers_share_one_refresh() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_delay(std::time::Duration::from_millis(50))
                    .set_body_json(serde_json::json!({
                        "access_token": "new-access", "refresh_token": "r", "expires_in": 3600, "token_type": "Bearer"
                    })),
            )
            .expect(1)
            .mount(&server)
            .await;
        let src = Arc::new(source(&server.uri(), stored(0), 1_000));
        let tasks: Vec<_> = (0..8)
            .map(|_| {
                let src = src.clone();
                tokio::spawn(async move { src.access_token().await.unwrap() })
            })
            .collect();
        for task in tasks {
            assert_eq!(task.await.unwrap(), "new-access");
        }
    }
}
