//! LACS E2E test CLI.
//!
//! Two modes:
//!   - Plan mode (default): reads a natural-language intent from stdin,
//!     connects to a running lacs-daemon for state queries, calls the LLM
//!     planner, and prints the resulting plan as JSON on stdout.
//!   - Doctor mode (`--doctor`): runs a sequence of health checks against
//!     the same stack (daemon socket, Ollama reachability, model presence,
//!     resolved thinking mode, `num_predict`) and prints one line per check.
//!
//! Environment variables:
//!   LACS_LISTEN_URI   — daemon socket (default: unix:///tmp/lacs-daemon.sock)
//!   LACS_LLM_PROVIDER — provider name (default: ollama)
//!   LACS_LLM_MODEL    — model override (default: provider's default)
//!   LACS_OLLAMA_URL   — Ollama base URL (default: http://localhost:11434)
//!   LACS_OLLAMA_THINK — override auto-detected thinking mode ("true"|"false")
//!
//! Exit codes:
//!   0 — plan produced (plan mode) / all checks green (doctor mode)
//!   1 — planning failed / any check red
//!   2 — usage / configuration error

use std::io::{self, Read};
use std::process;
use std::time::Duration;

use lacs_brain::config::BrainConfig;
use lacs_brain::planner::{
    resolve_ollama_think, LlmPlanner, PlanningError, OLLAMA_NUM_PREDICT, THINKING_MODEL_PREFIXES,
};
use lacs_brain::state_client::{CuratedState, StateClient};
use lacs_core::DEFAULT_LISTEN_URI;

// ---------------------------------------------------------------------------
// Synchronous IPC client (mirrors the shell's DaemonIpcClient)
// ---------------------------------------------------------------------------

struct TestDaemonClient {
    socket_path: String,
}

impl TestDaemonClient {
    fn new(socket_path: String) -> Self {
        Self { socket_path }
    }
}

impl StateClient for TestDaemonClient {
    fn curated_state(&self) -> Result<CuratedState, PlanningError> {
        use std::os::unix::net::UnixStream;
        use std::time::Duration;

        let mut stream = UnixStream::connect(&self.socket_path)
            .map_err(|e| PlanningError::StateUnavailable(format!("connect: {e}")))?;
        stream.set_read_timeout(Some(Duration::from_secs(10))).ok();
        stream.set_write_timeout(Some(Duration::from_secs(10))).ok();

        let req = serde_json::to_vec(&serde_json::json!({
            "type": "query_state",
            "request_id": "test-cli-state"
        }))
        .expect("static JSON");

        write_framed(&mut stream, &req)
            .map_err(|e| PlanningError::StateUnavailable(format!("send: {e}")))?;
        let msg = read_framed(&mut stream)
            .map_err(|e| PlanningError::StateUnavailable(format!("recv: {e}")))?;

        let resp: serde_json::Value = serde_json::from_slice(&msg)
            .map_err(|e| PlanningError::StateUnavailable(format!("parse: {e}")))?;

        match resp["type"].as_str() {
            Some("state_response") => {
                let s = resp
                    .get("state")
                    .ok_or_else(|| PlanningError::StateUnavailable("missing state".into()))?;
                let host = s["host_name"]
                    .as_str()
                    .ok_or_else(|| PlanningError::StateUnavailable("missing host_name".into()))?;
                let deployment = s["deployment"].as_str().unwrap_or("");
                CuratedState::new(
                    host,
                    deployment,
                    string_array(&s["services"]),
                    string_array(&s["flatpaks"]),
                    string_array(&s["toolboxes"]),
                    string_array(&s["layered_packages"]),
                    string_array(&s["containers"]),
                    string_array(&s["users"]),
                )
                .map_err(PlanningError::StateUnavailable)
            }
            Some("error_response") => Err(PlanningError::StateUnavailable(format!(
                "daemon error: {}",
                resp["message"].as_str().unwrap_or("unknown")
            ))),
            _ => Err(PlanningError::StateUnavailable(
                "unexpected response type".into(),
            )),
        }
    }

    fn query_action(
        &self,
        action_name: &str,
        params: &serde_json::Value,
    ) -> Result<String, PlanningError> {
        use std::os::unix::net::UnixStream;
        use std::time::Duration;

        let mut stream = UnixStream::connect(&self.socket_path)
            .map_err(|e| PlanningError::StateUnavailable(format!("connect: {e}")))?;
        stream.set_read_timeout(Some(Duration::from_secs(10))).ok();
        stream.set_write_timeout(Some(Duration::from_secs(10))).ok();

        let req = serde_json::to_vec(&serde_json::json!({
            "type": "query_action",
            "request_id": format!("test-cli-{action_name}"),
            "action_name": action_name,
            "params": params,
        }))
        .map_err(|e| PlanningError::StateUnavailable(format!("serialize: {e}")))?;

        write_framed(&mut stream, &req)
            .map_err(|e| PlanningError::StateUnavailable(format!("send: {e}")))?;
        let msg = read_framed(&mut stream)
            .map_err(|e| PlanningError::StateUnavailable(format!("recv: {e}")))?;

        let resp: serde_json::Value = serde_json::from_slice(&msg)
            .map_err(|e| PlanningError::StateUnavailable(format!("parse: {e}")))?;

        match resp["type"].as_str() {
            Some("query_action_response") => Ok(resp["output"].as_str().unwrap_or("").to_string()),
            Some("error_response") => Err(PlanningError::StateUnavailable(format!(
                "daemon error: {}",
                resp["message"].as_str().unwrap_or("unknown")
            ))),
            _ => Err(PlanningError::StateUnavailable(
                "unexpected response type".into(),
            )),
        }
    }
}

// ---------------------------------------------------------------------------
// Framing helpers (4-byte LE length prefix + UTF-8 JSON body)
// ---------------------------------------------------------------------------

fn write_framed(stream: &mut std::os::unix::net::UnixStream, msg: &[u8]) -> io::Result<()> {
    use std::io::Write;
    let len = u32::try_from(msg.len())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "message too large"))?;
    stream.write_all(&len.to_le_bytes())?;
    stream.write_all(msg)
}

fn read_framed(stream: &mut std::os::unix::net::UnixStream) -> io::Result<Vec<u8>> {
    use std::io::Read;
    let mut len_buf = [0u8; 4];
    stream.read_exact(&mut len_buf)?;
    let len = u32::from_le_bytes(len_buf);
    if len > 4 * 1024 * 1024 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "response too large",
        ));
    }
    let mut buf = vec![0u8; len as usize];
    stream.read_exact(&mut buf)?;
    Ok(buf)
}

fn string_array(v: &serde_json::Value) -> Vec<String> {
    v.as_array()
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default()
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

#[tokio::main]
async fn main() {
    // Initialise tracing so RUST_LOG works (e.g. RUST_LOG=rig=debug).
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_writer(std::io::stderr)
        .init();

    // Dispatch on argv. Doctor mode skips stdin and runs health checks.
    let args: Vec<String> = std::env::args().collect();
    let is_doctor = args.iter().any(|a| a == "--doctor" || a == "doctor");
    let is_help = args.iter().any(|a| a == "--help" || a == "-h");

    if is_help {
        print_usage();
        process::exit(0);
    }

    if is_doctor {
        process::exit(run_doctor().await);
    }

    process::exit(run_plan().await);
}

fn print_usage() {
    eprintln!(
        "usage:\n  \
         echo 'intent' | lacs-test-cli   # plan mode\n  \
         lacs-test-cli --doctor          # health checks\n  \
         lacs-test-cli --help            # this message"
    );
}

// ---------------------------------------------------------------------------
// Plan mode
// ---------------------------------------------------------------------------

async fn run_plan() -> i32 {
    // Read intent from stdin.
    let mut intent = String::new();
    if let Err(e) = io::stdin().read_to_string(&mut intent) {
        eprintln!("lacs-test-cli: failed to read stdin: {e}");
        return 2;
    }
    let intent = intent.trim();
    if intent.is_empty() {
        eprintln!("lacs-test-cli: no intent provided on stdin");
        print_usage();
        return 2;
    }

    // Resolve daemon socket path from env or default.
    let listen_uri =
        std::env::var("LACS_LISTEN_URI").unwrap_or_else(|_| DEFAULT_LISTEN_URI.to_string());
    let socket_path = listen_uri
        .strip_prefix("unix://")
        .unwrap_or(&listen_uri)
        .to_string();

    // Build brain config from env (defaults to Ollama).
    let config = match BrainConfig::from_env() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("lacs-test-cli: config error: {e}");
            return 2;
        }
    };

    eprintln!(
        "lacs-test-cli: provider={}, model={}, socket={}",
        config.provider_name(),
        config.model_name(),
        socket_path
    );

    // Construct planner.
    let state_client = Box::new(TestDaemonClient::new(socket_path));
    let planner = match LlmPlanner::from_config(config, state_client) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("lacs-test-cli: planner init failed: {e}");
            return 2;
        }
    };

    // Run the planning loop.
    eprintln!("lacs-test-cli: planning intent: {intent}");
    match planner.plan_intent(intent).await {
        Ok(plan) => {
            let json = serde_json::to_string_pretty(&plan).expect("Plan is always serialisable");
            println!("{json}");
            0
        }
        Err(PlanningError::EmptyIntent) => {
            eprintln!("lacs-test-cli: empty intent");
            2
        }
        Err(e) => {
            eprintln!("lacs-test-cli: planning failed: {e}");
            1
        }
    }
}

// ---------------------------------------------------------------------------
// Doctor mode
// ---------------------------------------------------------------------------

/// Outcome of a single doctor check. Binary state — we do not
/// distinguish "warn" from "err" in the current checks, and adding an
/// unused variant would just attract dead-code lints.
enum CheckStatus {
    Ok(String),
    Err(String),
}

impl CheckStatus {
    fn is_err(&self) -> bool {
        matches!(self, Self::Err(_))
    }
}

fn report(label: &str, status: &CheckStatus) {
    // Visible in monospace terminals, no external deps.
    let (marker, detail) = match status {
        CheckStatus::Ok(d) => ("[ ok ]", d.as_str()),
        CheckStatus::Err(d) => ("[fail]", d.as_str()),
    };
    println!("  {marker}  {label:<14} {detail}");
}

async fn run_doctor() -> i32 {
    // Apply config.toml defaults so the same resolution order the daemon
    // and shell use is applied here too — without this, a user whose
    // config.toml overrides the model would see the built-in default
    // (`DEFAULT_OLLAMA_MODEL`) in the doctor output.
    lacs_core::config::LacsConfig::load().apply_defaults_to_env();

    println!("lacs-test-cli doctor");
    let mut any_err = false;

    // 1. Config resolution
    let config = match BrainConfig::from_env() {
        Ok(c) => c,
        Err(e) => {
            report("config", &CheckStatus::Err(format!("{e}")));
            return 1;
        }
    };
    report(
        "config",
        &CheckStatus::Ok(format!(
            "provider={}, model={}",
            config.provider_name(),
            config.model_name()
        )),
    );

    // 2. Daemon reachable
    let listen_uri =
        std::env::var("LACS_LISTEN_URI").unwrap_or_else(|_| DEFAULT_LISTEN_URI.to_string());
    let socket_path = listen_uri
        .strip_prefix("unix://")
        .unwrap_or(&listen_uri)
        .to_string();
    let daemon_status = check_daemon(&socket_path);
    if daemon_status.is_err() {
        any_err = true;
    }
    report("daemon", &daemon_status);

    // 3. Ollama (if applicable)
    let is_ollama = config.provider_name() == "ollama";
    if is_ollama {
        let ollama_url =
            std::env::var("LACS_OLLAMA_URL").unwrap_or_else(|_| "http://localhost:11434".into());
        let model = config.model_name().to_string();
        let (ollama_status, model_status) = check_ollama(&ollama_url, &model).await;
        if ollama_status.is_err() {
            any_err = true;
        }
        // Model missing is fatal — a planner cannot succeed without it —
        // but only flag if Ollama itself is up; otherwise the Ollama fail
        // already captures the root cause.
        if model_status.is_err() && !ollama_status.is_err() {
            any_err = true;
        }
        report("ollama", &ollama_status);
        report("model", &model_status);

        // 4. Thinking-mode resolution
        let think = resolve_ollama_think(&model);
        let source = think_source(&model);
        report(
            "thinking",
            &CheckStatus::Ok(format!(
                "{} ({})",
                if think { "enabled" } else { "disabled" },
                source
            )),
        );

        // 5. num_predict
        report(
            "num_predict",
            &CheckStatus::Ok(format!("{} (options.num_predict)", OLLAMA_NUM_PREDICT)),
        );
    } else {
        // Cloud providers don't need Ollama; note which key is expected
        // but don't fail on its presence here — API-key validation is
        // handled by `BrainConfig::from_env` (which succeeded above).
        report(
            "ollama",
            &CheckStatus::Ok(format!("skipped (provider = {})", config.provider_name())),
        );
    }

    println!();
    if any_err {
        println!("doctor: FAIL — one or more checks are red. Fix them and re-run.");
        1
    } else {
        println!("doctor: all checks green.");
        0
    }
}

/// Ping the daemon socket with a `query_state` message and accept any
/// response type — we only care that the socket exists and accepts a
/// round-trip.
fn check_daemon(socket_path: &str) -> CheckStatus {
    use std::os::unix::net::UnixStream;

    let mut stream = match UnixStream::connect(socket_path) {
        Ok(s) => s,
        Err(e) => {
            return CheckStatus::Err(format!(
                "cannot connect to {socket_path}: {e} (is lacs-daemon running?)"
            ));
        }
    };
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(5)));

    let req = serde_json::to_vec(&serde_json::json!({
        "type": "query_state",
        "request_id": "doctor"
    }))
    .expect("static JSON");
    if let Err(e) = write_framed(&mut stream, &req) {
        return CheckStatus::Err(format!("send failed: {e}"));
    }
    match read_framed(&mut stream) {
        Ok(_) => CheckStatus::Ok(format!("reachable at {socket_path}")),
        Err(e) => CheckStatus::Err(format!("no reply from {socket_path}: {e}")),
    }
}

/// Query `{ollama_url}/api/tags` to confirm Ollama is up, then check
/// whether `model` appears in the returned tag list.
async fn check_ollama(ollama_url: &str, model: &str) -> (CheckStatus, CheckStatus) {
    let url = format!("{}/api/tags", ollama_url.trim_end_matches('/'));
    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            return (
                CheckStatus::Err(format!("reqwest build failed: {e}")),
                CheckStatus::Err("skipped (ollama unreachable)".into()),
            );
        }
    };

    let resp = match client.get(&url).send().await {
        Ok(r) => r,
        Err(e) => {
            return (
                CheckStatus::Err(format!("cannot reach {ollama_url}: {e}")),
                CheckStatus::Err("skipped (ollama unreachable)".into()),
            );
        }
    };

    if !resp.status().is_success() {
        return (
            CheckStatus::Err(format!("{} returned HTTP {}", ollama_url, resp.status())),
            CheckStatus::Err("skipped (ollama unhealthy)".into()),
        );
    }

    let body: serde_json::Value = match resp.json().await {
        Ok(v) => v,
        Err(e) => {
            return (
                CheckStatus::Err(format!("malformed /api/tags response: {e}")),
                CheckStatus::Err("skipped (ollama unhealthy)".into()),
            );
        }
    };

    let ollama_status = CheckStatus::Ok(format!("reachable at {ollama_url}"));

    // Ollama returns models as `{"models": [{"name": "qwen3:8b", ...}, ...]}`.
    // Match on exact name or `name + ":latest"`, to mirror Ollama's own
    // implicit-tag behaviour.
    let names: Vec<&str> = body["models"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|m| m.get("name").and_then(|n| n.as_str()))
                .collect()
        })
        .unwrap_or_default();

    let model_with_latest = format!("{model}:latest");
    let model_found = names.iter().any(|n| *n == model || *n == model_with_latest);
    let model_status = if model_found {
        CheckStatus::Ok(format!("'{model}' is pulled"))
    } else if names.is_empty() {
        CheckStatus::Err(format!("'{model}' is not pulled (no models available)"))
    } else {
        CheckStatus::Err(format!(
            "'{model}' is not pulled (available: {})",
            names.join(", ")
        ))
    };

    (ollama_status, model_status)
}

/// Human-readable explanation of *why* a given model gets its thinking
/// decision — either an env-var override or the auto-detection heuristic.
fn think_source(model: &str) -> String {
    if let Ok(raw) = std::env::var("LACS_OLLAMA_THINK") {
        let v = raw.trim().to_lowercase();
        if v == "true" || v == "false" {
            return format!("LACS_OLLAMA_THINK={v}");
        }
    }
    let model_lower = model.to_lowercase();
    if let Some(prefix) = THINKING_MODEL_PREFIXES
        .iter()
        .find(|p| model_lower.starts_with(*p))
    {
        format!("auto: model starts with '{prefix}'")
    } else {
        "auto: model not in thinking-prefix list".into()
    }
}
