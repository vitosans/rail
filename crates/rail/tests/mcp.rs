//! The real `rail mcp` binary driven over piped stdio by an rmcp client. The client plays an
//! MCP host that accepts, declines, cancels, answers oddly, or cannot show elicitation forms.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use rmcp::model::{
    CallToolRequest, CallToolRequestParams, CallToolResult, ClientCapabilities, ClientConfig,
    ClientRequest, ElicitRequestParams, ElicitResult, ElicitationAction, GetPromptRequestParams,
    Implementation,
};
use rmcp::service::{PeerRequestOptions, RequestContext, RunningService, ServiceError};
use rmcp::transport::TokioChildProcess;
use rmcp::{ClientHandler, ErrorData, RoleClient, ServiceExt};
use serde_json::{Value, json};

const FILE_BACKEND: &str = "[general]\nsecrets = \"file\"\n";
const KEYRING_BACKEND: &str = "[general]\nsecrets = \"keyring\"\n";
const WORK: &str = "[general]\nsecrets = \"file\"\n\n[[accounts]]\nname = \"work\"\nkind = \"gmail\"\naddress = \"me@example.com\"\nclient_id = \"cid\"\nclient_secret = \"GOCSPX-test-secret\"\n";
const CLIENT_SECRET: &str = "GOCSPX-test-secret";
/// A secret an agent pastes where it does not belong.
const CANARY: &str = "GOCSPX-canary-pasted-secret";
const ACCESS: &str = "test-access-token";
const REFRESH: &str = "test-refresh-token";
const CODE: &str = "test-authorization-code";
/// The longest any one step of a test may take before it counts as hung.
const BOUND: Duration = Duration::from_secs(30);

/// A tool's (read-only, destructive, open-world) hints.
type Hints = (Option<bool>, Option<bool>, Option<bool>);

/// What the host does with a confirmation form.
#[derive(Clone, Copy, Debug)]
enum Reply {
    /// Accept with the box ticked.
    Yes,
    /// Accept with the box left unticked.
    Unticked,
    /// Accept without any content.
    Empty,
    Decline,
    Cancel,
}

/// An MCP host. `reply` is what it does with an elicitation form; `None` means it has no
/// elicitation capability at all. `meanwhile` runs before it answers, to change things
/// between the question and the answer.
#[derive(Clone)]
struct Host {
    reply: Option<Reply>,
    asked: Arc<Mutex<Vec<String>>>,
    meanwhile: Option<Arc<dyn Fn() + Send + Sync>>,
}

impl ClientHandler for Host {
    fn get_info(&self) -> ClientConfig {
        let capabilities = match self.reply {
            Some(_) => ClientCapabilities::builder().enable_elicitation().build(),
            None => ClientCapabilities::default(),
        };
        ClientConfig::new(capabilities, Implementation::new("rail-test-host", "0"))
    }

    async fn create_elicitation(
        &self,
        request: ElicitRequestParams,
        _: RequestContext<RoleClient>,
    ) -> Result<ElicitResult, ErrorData> {
        if let ElicitRequestParams::FormElicitationParams { message, .. } = request {
            self.asked.lock().unwrap().push(message);
        }
        if let Some(meanwhile) = &self.meanwhile {
            meanwhile();
        }
        let accept = ElicitResult::new(ElicitationAction::Accept);
        Ok(match self.reply {
            Some(Reply::Yes) => accept.with_content(json!({"confirm": true})),
            Some(Reply::Unticked) => accept.with_content(json!({"confirm": false})),
            Some(Reply::Empty) => accept,
            Some(Reply::Cancel) => ElicitResult::new(ElicitationAction::Cancel),
            Some(Reply::Decline) | None => ElicitResult::new(ElicitationAction::Decline),
        })
    }
}

fn host(reply: Option<Reply>) -> Host {
    Host {
        reply,
        asked: Arc::default(),
        meanwhile: None,
    }
}

fn write_config(dir: &tempfile::TempDir, text: &str) {
    let config = dir.path().join("config");
    std::fs::create_dir_all(&config).unwrap();
    std::fs::write(config.join("config.toml"), text).unwrap();
}

/// A browser stand-in: writes the URL it was given to `$RAIL_TEST_URL_FILE` and exits.
fn write_opener(dir: &tempfile::TempDir) -> std::path::PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let path = dir.path().join("opener.sh");
    let script = "#!/bin/sh\nprintf '%s' \"$1\" > \"$RAIL_TEST_URL_FILE.tmp\"\nmv \"$RAIL_TEST_URL_FILE.tmp\" \"$RAIL_TEST_URL_FILE\"\n";
    std::fs::write(&path, script).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    path
}

/// Point a command's directories under `dir` and give it the stand-in browser, so nothing a
/// test starts (the MCP server, the daemon it spawns) touches the user's files or browser.
fn rail_env<'a>(
    command: &'a mut tokio::process::Command,
    dir: &tempfile::TempDir,
) -> &'a mut tokio::process::Command {
    command
        .env("RAIL_CONFIG_DIR", dir.path().join("config"))
        .env("RAIL_DATA_DIR", dir.path().join("data"))
        .env("RAIL_CACHE_DIR", dir.path().join("cache"))
        .env("RAIL_STATE_DIR", dir.path().join("state"))
        .env("RAIL_RUNTIME_DIR", dir.path().join("run"))
        .env("XDG_CONFIG_HOME", dir.path().join("xdg"))
        // On macOS the launchd agent lives under HOME, so there HOME is the test's too.
        .envs(cfg!(target_os = "macos").then(|| ("HOME", dir.path().join("home"))))
        .env("RAIL_TEST_IDLE_SECS", "2")
        .env("RAIL_TEST_OPENER", write_opener(dir))
        .env("RAIL_TEST_URL_FILE", dir.path().join("url.txt"))
        // A configured account in a test must never reach Google or Microsoft, and its daemon must still
        // idle out. `start` sets RAIL_TEST_TOKEN_URL again when a test serves one.
        .env(
            "RAIL_TEST_GMAIL_URL",
            "http://127.0.0.1:9/gmail/v1/users/me/",
        )
        .env("RAIL_TEST_GRAPH_URL", "http://127.0.0.1:9/v1.0/")
        .env("RAIL_TEST_TOKEN_URL", "http://127.0.0.1:9/token")
        .env("RAIL_TEST_IDLE_WITH_ACCOUNTS", "1")
}

/// Runs `rail daemon stop` for its directory when dropped, so the daemon `rail mcp` started
/// ends with the test, even when the test fails half way.
struct StopsDaemon(std::path::PathBuf);

impl Drop for StopsDaemon {
    fn drop(&mut self) {
        let dir = &self.0;
        let _ = std::process::Command::new(env!("CARGO_BIN_EXE_rail"))
            .args(["daemon", "stop"])
            .env("RAIL_CONFIG_DIR", dir.join("config"))
            .env("RAIL_DATA_DIR", dir.join("data"))
            .env("RAIL_CACHE_DIR", dir.join("cache"))
            .env("RAIL_STATE_DIR", dir.join("state"))
            .env("RAIL_RUNTIME_DIR", dir.join("run"))
            .env("RAIL_TEST_OPENER", dir.join("opener.sh"))
            .env("RAIL_TEST_URL_FILE", dir.join("url.txt"))
            .output();
    }
}

/// A running `rail mcp` and the guard that stops its daemon.
struct Session {
    client: RunningService<RoleClient, Host>,
    _daemon: StopsDaemon,
}

impl std::ops::Deref for Session {
    type Target = RunningService<RoleClient, Host>;
    fn deref(&self) -> &Self::Target {
        &self.client
    }
}

impl Session {
    async fn close(self) {
        let Session { client, _daemon } = self;
        tokio::time::timeout(BOUND, client.cancel())
            .await
            .expect("rail mcp stopped in time")
            .unwrap();
    }
}

/// Start `rail mcp` confined to `dir`, with the test hooks pointing at `token_url`.
async fn start(dir: &tempfile::TempDir, host: Host, token_url: Option<String>) -> Session {
    start_with_gmail(dir, host, token_url, None).await
}

/// `start`, with the daemon's Gmail at `gmail_url` (a fake) when given.
async fn start_with_gmail(
    dir: &tempfile::TempDir,
    host: Host,
    token_url: Option<String>,
    gmail_url: Option<String>,
) -> Session {
    let mut command = tokio::process::Command::new(env!("CARGO_BIN_EXE_rail"));
    rail_env(command.arg("mcp"), dir);
    if let Some(gmail_url) = gmail_url {
        command.env("RAIL_TEST_GMAIL_URL", gmail_url);
    }
    if let Some(token_url) = token_url {
        command.env("RAIL_TEST_TOKEN_URL", token_url);
    }
    let daemon = StopsDaemon(dir.path().to_path_buf());
    let transport = TokioChildProcess::new(command).unwrap();
    let client = tokio::time::timeout(BOUND, host.serve(transport))
        .await
        .expect("the handshake finished in time")
        .unwrap();
    Session {
        client,
        _daemon: daemon,
    }
}

/// A tool call, bounded; the error is the protocol's (an argument the schema refuses).
async fn try_call(
    client: &Session,
    name: &str,
    args: Value,
) -> Result<CallToolResult, ServiceError> {
    let arguments = args.as_object().cloned().unwrap_or_default();
    let request = CallToolRequestParams::new(name.to_string()).with_arguments(arguments);
    tokio::time::timeout(BOUND, client.call_tool(request))
        .await
        .unwrap_or_else(|_| panic!("{name} answered in time"))
}

async fn call(client: &Session, name: &str, args: Value) -> CallToolResult {
    try_call(client, name, args).await.unwrap()
}

/// `future`, failing the test if it takes longer than `BOUND`.
async fn bounded<T>(what: &str, future: impl std::future::Future<Output = T>) -> T {
    tokio::time::timeout(BOUND, future)
        .await
        .unwrap_or_else(|_| panic!("{what} finished in time"))
}

/// True when rmcp refused the arguments before the tool ran: a field the schema does not have.
fn refused_by_schema(result: &CallToolResult) -> bool {
    result.is_error == Some(true)
        && result.structured_content.is_none()
        && text(result).contains("unknown field")
}

fn structured(result: &CallToolResult) -> &Value {
    result
        .structured_content
        .as_ref()
        .expect("structured content")
}

fn text(result: &CallToolResult) -> String {
    serde_json::to_string(&result.content).unwrap()
}

#[tokio::test]
async fn the_tools_carry_their_hints_and_no_raw_secret_input() {
    let dir = tempfile::tempdir().unwrap();
    write_config(&dir, FILE_BACKEND);
    let client = start(&dir, host(None), None).await;
    let tools = bounded("tools/list", client.list_all_tools())
        .await
        .unwrap();
    let hints: BTreeMap<String, Hints> = tools
        .iter()
        .map(|t| {
            let a = t.annotations.clone().unwrap_or_default();
            (
                t.name.to_string(),
                (a.read_only_hint, a.destructive_hint, a.open_world_hint),
            )
        })
        .collect();
    let read_only = |name: &str, open: bool| (name.to_string(), (Some(true), None, Some(open)));
    let writes = |name: &str, destructive: bool, open: bool| {
        (
            name.to_string(),
            (Some(false), Some(destructive), Some(open)),
        )
    };
    let expected: BTreeMap<_, _> = [
        read_only("rail_status", false),
        read_only("rail_accounts_list", false),
        read_only("rail_presets_list", false),
        read_only("rail_preset_fetch", true),
        read_only("rail_provider_guide", false),
        writes("rail_account_add", false, true),
        writes("rail_account_login", false, true),
        writes("rail_admin_consent", false, true),
        read_only("rail_wait", false),
        writes("rail_cancel", false, false),
        writes("rail_secrets_configure", false, false),
        writes("rail_account_remove", true, false),
        read_only("rail_sync_status", false),
        writes("rail_sync_now", false, true),
    ]
    .into_iter()
    .collect();
    assert_eq!(hints, expected);
    let schema = |name: &str| {
        let tool = tools.iter().find(|t| t.name == name).unwrap();
        serde_json::to_value(&tool.input_schema).unwrap()
    };
    let add: BTreeSet<String> = schema("rail_account_add")["properties"]
        .as_object()
        .unwrap()
        .keys()
        .cloned()
        .collect();
    assert!(!add.contains("client_secret"), "{add:?}");
    assert!(add.contains("client_secret_file") && add.contains("client_secret_ref"));
    assert_eq!(schema("rail_account_add")["additionalProperties"], false);
    let remove = schema("rail_account_remove").to_string();
    assert!(
        !remove.contains("confirm"),
        "the agent cannot confirm for the user: {remove}"
    );
    let configure = schema("rail_secrets_configure").to_string();
    assert!(!configure.contains("op_path"), "{configure}");
    assert!(
        !configure.contains("confirm"),
        "the agent cannot confirm for the user: {configure}"
    );
    let now = schema("rail_sync_now").to_string();
    assert!(
        !now.contains("watch"),
        "an agent cannot subscribe the server: {now}"
    );
    assert_eq!(schema("rail_sync_now")["additionalProperties"], false);
    assert_eq!(schema("rail_sync_status")["additionalProperties"], false);
    client.close().await;
}

/// Spec section 3: agents cannot change mail in this phase. No tool runs an action operation,
/// no tool is named like one, and no tool's schema offers one. The tool list is pinned.
#[tokio::test]
async fn no_tool_changes_mail() {
    let dir = tempfile::tempdir().unwrap();
    write_config(&dir, FILE_BACKEND);
    let client = start(&dir, host(None), None).await;
    let tools = bounded("tools/list", client.list_all_tools())
        .await
        .unwrap();
    let names: BTreeSet<String> = tools.iter().map(|t| t.name.to_string()).collect();
    let expected: BTreeSet<String> = [
        "rail_status",
        "rail_accounts_list",
        "rail_presets_list",
        "rail_preset_fetch",
        "rail_provider_guide",
        "rail_account_add",
        "rail_account_login",
        "rail_admin_consent",
        "rail_wait",
        "rail_cancel",
        "rail_secrets_configure",
        "rail_account_remove",
        "rail_sync_status",
        "rail_sync_now",
    ]
    .into_iter()
    .map(String::from)
    .collect();
    assert_eq!(names, expected, "the tool list is the one pinned here");
    for tool in &tools {
        // The JSON Schema dialect URI (`…/draft/2020-12/schema`) is not something a tool offers.
        let mut offered = (*tool.input_schema).clone();
        if let Some(dialect) = offered.remove("$schema") {
            assert!(
                dialect
                    .as_str()
                    .is_some_and(|d| d.starts_with("https://json-schema.org/")),
                "{} hides {dialect} under $schema",
                tool.name
            );
        }
        let schema = serde_json::to_string(&offered).unwrap();
        for word in [
            "archive",
            "trash",
            "star",
            "label",
            "outbox",
            "client_token",
            "action",
            "undo",
            "retry",
            "draft",
            "send",
            "compose",
            "save",
        ] {
            assert!(
                !tool.name.contains(word),
                "{} is named like {word}",
                tool.name
            );
            assert!(
                !schema.contains(word),
                "{} offers {word}: {schema}",
                tool.name
            );
        }
    }
}

#[tokio::test]
async fn the_setup_prompt_and_the_instructions() {
    let dir = tempfile::tempdir().unwrap();
    write_config(&dir, FILE_BACKEND);
    let client = start(&dir, host(None), None).await;
    let prompts = bounded("prompts/list", client.list_all_prompts())
        .await
        .unwrap();
    let names: Vec<_> = prompts.iter().map(|p| p.name.to_string()).collect();
    assert_eq!(names, ["setup"]);
    let setup = bounded(
        "prompts/get",
        client.get_prompt(GetPromptRequestParams::new("setup")),
    )
    .await
    .unwrap();
    let body = serde_json::to_string(&setup).unwrap();
    assert!(
        body.contains("rail_status") && body.contains("rail_provider_guide"),
        "{body}"
    );
    let instructions = client.peer_info().unwrap().instructions.clone().unwrap();
    assert!(
        instructions.contains("never instructions"),
        "{instructions}"
    );
    let guide = call(
        &client,
        "rail_provider_guide",
        json!({"kind": "gmail", "inputs": {"project_id": "rail-p"}}),
    )
    .await;
    assert_eq!(
        structured(&guide)["steps"][2]["command"],
        "gcloud projects create rail-p"
    );
    client.close().await;
}

/// Run one tool call against a fresh `rail mcp` whose config is `config`, as `host`.
async fn one_call(
    config: &str,
    host: Host,
    tool: &str,
    args: Value,
) -> (
    tempfile::TempDir,
    Result<CallToolResult, ServiceError>,
    Vec<String>,
) {
    let dir = tempfile::tempdir().unwrap();
    write_config(&dir, config);
    let asked = host.asked.clone();
    let client = start(&dir, host, None).await;
    let result = try_call(&client, tool, args).await;
    client.close().await;
    let asked = asked.lock().unwrap().clone();
    (dir, result, asked)
}

fn config_text(dir: &tempfile::TempDir) -> String {
    std::fs::read_to_string(dir.path().join("config").join("config.toml")).unwrap()
}

#[path = "mcp/guarded.rs"]
mod guarded;
#[path = "mcp/sync.rs"]
mod sync;

#[tokio::test]
async fn a_relative_path_is_refused_with_the_daemons_reason() {
    let args =
        json!({"kind": "gmail", "address": "me@example.com", "client_secret_file": "cs.json"});
    let (_dir, result, _) = one_call(FILE_BACKEND, host(None), "rail_account_add", args).await;
    let result = result.unwrap();
    assert_eq!(result.is_error, Some(true));
    assert!(
        text(&result).contains("use an absolute path"),
        "{}",
        text(&result)
    );
}

/// A client secret from an agent is refused with fixed text, and a pasted secret
/// in the reference field is never echoed.
#[tokio::test]
async fn a_client_secret_from_an_agent_is_refused_and_never_echoed() {
    let dir = tempfile::tempdir().unwrap();
    write_config(&dir, FILE_BACKEND);
    let client = start(&dir, host(None), None).await;
    let pasted = json!({"kind": "gmail", "address": "me@example.com", "client_id": "c", "client_secret": CANARY});
    let refused = call(&client, "rail_account_add", pasted).await;
    assert_eq!(refused.is_error, Some(true));
    assert!(
        text(&refused).contains("Rail does not take client secrets from agents"),
        "{}",
        text(&refused)
    );
    let as_ref = json!({"kind": "gmail", "address": "me@example.com", "client_id": "c", "client_secret_ref": CANARY});
    let refused_ref = call(&client, "rail_account_add", as_ref).await;
    assert_eq!(refused_ref.is_error, Some(true));
    let unknown = json!({"kind": "gmail", "address": "me@example.com", "secret": CANARY});
    let refused_unknown = call(&client, "rail_account_add", unknown).await;
    assert!(
        refused_by_schema(&refused_unknown),
        "{}",
        text(&refused_unknown)
    );
    client.close().await;
    let log = std::fs::read_to_string(dir.path().join("state").join("rail.log")).unwrap();
    for result in [&refused, &refused_ref, &refused_unknown] {
        let body = serde_json::to_string(result).unwrap();
        assert!(!body.contains(CANARY), "{body}");
    }
    assert!(!log.contains(CANARY), "the pasted secret is in rail.log");
    assert!(!config_text(&dir).contains(CANARY));
}

/// Every tool refuses arguments it does not know, the ones without parameters included.
#[tokio::test]
async fn a_tool_without_parameters_refuses_unknown_arguments() {
    let dir = tempfile::tempdir().unwrap();
    write_config(&dir, FILE_BACKEND);
    let client = start(&dir, host(None), None).await;
    let refused = call(&client, "rail_status", json!({"verbose": true})).await;
    assert!(refused_by_schema(&refused), "{}", text(&refused));
    let fine = call(&client, "rail_status", json!({})).await;
    assert_ne!(fine.is_error, Some(true), "{}", text(&fine));
    client.close().await;
}

/// Rmcp's text for a wrong-typed argument quotes the value, which may be a pasted
/// secret; it goes back to the agent that sent it, never into `rail.log`.
#[tokio::test]
async fn a_wrong_typed_argument_never_reaches_the_log() {
    let dir = tempfile::tempdir().unwrap();
    write_config(&dir, FILE_BACKEND);
    let client = start(&dir, host(None), None).await;
    let wrong = json!({"id": "add-1", "timeout_secs": CANARY});
    let refused = call(&client, "rail_wait", wrong).await;
    assert_eq!(refused.is_error, Some(true), "{}", text(&refused));
    client.close().await;
    let log = std::fs::read_to_string(dir.path().join("state").join("rail.log")).unwrap();
    assert!(
        !log.contains(CANARY),
        "the wrong-typed value is in rail.log:\n{log}"
    );
}

/// The host giving up on one `rail_wait` (MCP `notifications/cancelled`) leaves the login
/// running: only `rail_cancel` cancels it.
#[tokio::test]
async fn cancelling_a_wait_request_does_not_cancel_the_login() {
    let dir = tempfile::tempdir().unwrap();
    write_config(&dir, WORK);
    let client = start(&dir, host(None), None).await;
    let begun = call(&client, "rail_account_login", json!({"account": "work"})).await;
    let id = structured(&begun)["id"].as_str().unwrap().to_string();
    let wait = CallToolRequestParams::new("rail_wait")
        .with_arguments(json!({"id": id}).as_object().cloned().unwrap());
    let request = ClientRequest::CallToolRequest(CallToolRequest::new(wait));
    let handle = bounded(
        "the wait request",
        client.send_cancellable_request(request, PeerRequestOptions::no_options()),
    )
    .await
    .unwrap();
    tokio::time::sleep(Duration::from_millis(200)).await;
    bounded(
        "the cancellation",
        handle.cancel(Some("the host gave up".into())),
    )
    .await
    .unwrap();
    let still = call(&client, "rail_wait", json!({"id": id, "timeout_secs": 1})).await;
    assert_eq!(
        structured(&still)["state"]["state"],
        "waiting",
        "{}",
        text(&still)
    );
    let cancelled = call(&client, "rail_cancel", json!({"id": id})).await;
    assert_eq!(structured(&cancelled)["state"]["state"], "cancelled");
    client.close().await;
}

async fn token_server() -> wiremock::MockServer {
    use wiremock::matchers::{body_string_contains, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/token"))
        .and(body_string_contains(format!("code={CODE}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "access_token": ACCESS, "refresh_token": REFRESH,
            "expires_in": 3600, "token_type": "Bearer"
        })))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/gmail/v1/users/me/profile"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"emailAddress": "me@example.com"})),
        )
        .mount(&server)
        .await;
    server
}

/// Wait for the opener to record the login URL, then follow its redirect with the code.
async fn finish_in_the_browser(dir: &tempfile::TempDir) {
    let file = dir.path().join("url.txt");
    let mut url = None;
    for _ in 0..500 {
        if let Ok(text) = std::fs::read_to_string(&file) {
            url = Some(url::Url::parse(&text).unwrap());
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let url = url.expect("the browser was asked");
    std::fs::remove_file(&file).unwrap();
    let query = |key: &str| {
        url.query_pairs()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.into_owned())
            .unwrap()
    };
    let redirect = format!(
        "{}/?code={CODE}&state={}",
        query("redirect_uri"),
        query("state")
    );
    bounded("the redirect", reqwest::get(redirect))
        .await
        .unwrap();
}

/// Review focus 1: the add flow over MCP with a client secret file, then every other tool;
/// no result and no log line carries the client secret, a token or the authorization code.
#[cfg(debug_assertions)]
#[tokio::test]
async fn every_tool_result_and_the_log_are_free_of_secrets() {
    let dir = tempfile::tempdir().unwrap();
    write_config(&dir, FILE_BACKEND);
    let server = bounded("the token server", token_server()).await;
    let client = start_with_gmail(
        &dir,
        host(None),
        Some(format!("{}/token", server.uri())),
        Some(format!("{}/gmail/v1/users/me/", server.uri())),
    )
    .await;
    let file = dir.path().join("client_secret_1.json");
    let google = json!({"installed": {"client_id": "1.apps.googleusercontent.com", "client_secret": CLIENT_SECRET}});
    std::fs::write(&file, google.to_string()).unwrap();
    let preset = dir.path().join("acme.toml");
    std::fs::write(&preset, format!("name = \"Acme\"\nkind = \"gmail\"\nclient_id = \"acme\"\nclient_secret = \"{CLIENT_SECRET}\"\n")).unwrap();
    let mut results = Vec::new();

    let begun = call(
        &client,
        "rail_account_add",
        json!({"kind": "gmail", "address": "me@example.com", "client_secret_file": file}),
    )
    .await;
    let id = structured(&begun)["id"].as_str().unwrap().to_string();
    assert!(text(&begun).contains("rail_wait"), "{}", text(&begun));
    results.push(begun);
    finish_in_the_browser(&dir).await;
    let done = call(&client, "rail_wait", json!({"id": id})).await;
    assert_eq!(
        structured(&done)["state"]["state"],
        "done",
        "{}",
        text(&done)
    );
    results.push(done);
    let config = config_text(&dir);
    assert!(
        config.contains("client_id = \"1.apps.googleusercontent.com\""),
        "{config}"
    );

    for (tool, args) in [
        ("rail_status", json!({})),
        ("rail_accounts_list", json!({})),
        ("rail_presets_list", json!({})),
        ("rail_preset_fetch", json!({"source": preset})),
        ("rail_provider_guide", json!({"kind": "gmail"})),
        ("rail_provider_guide", json!({"kind": "m365"})),
        ("rail_secrets_configure", json!({"backend": "file"})),
        ("rail_secrets_configure", json!({"backend": "1password"})),
        ("rail_account_remove", json!({"account": "me@example.com"})),
        ("rail_wait", json!({"id": "add-999", "timeout_secs": 1})),
        (
            "rail_account_add",
            json!({"kind": "gmail", "address": "x@example.com", "client_secret_ref": CANARY}),
        ),
    ] {
        results.push(call(&client, tool, args).await);
    }
    for tool in ["rail_account_login", "rail_admin_consent"] {
        let args = match tool {
            "rail_account_login" => json!({"account": "me@example.com"}),
            _ => json!({"client_id": "cid", "tenant": "contoso.onmicrosoft.com"}),
        };
        let begun = call(&client, tool, args).await;
        let id = structured(&begun)["id"].as_str().unwrap().to_string();
        results.push(begun);
        results.push(call(&client, "rail_cancel", json!({"id": id})).await);
    }
    client.close().await;

    let log = std::fs::read_to_string(dir.path().join("state").join("rail.log")).unwrap();
    for secret in [CLIENT_SECRET, ACCESS, REFRESH, CODE, CANARY] {
        for result in &results {
            let body = serde_json::to_string(result).unwrap();
            assert!(!body.contains(secret), "{secret} in a tool result: {body}");
        }
        assert!(!log.contains(secret), "{secret} in rail.log");
    }
}
