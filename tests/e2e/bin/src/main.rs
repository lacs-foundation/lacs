//! LACS E2E test CLI.
//!
//! Reads a natural-language intent from stdin, connects to a running
//! lacs-daemon for state queries, calls the LLM planner via Ollama,
//! and prints the resulting plan as JSON on stdout.
//!
//! Environment variables:
//!   LACS_LISTEN_URI   — daemon socket (default: unix:///tmp/lacs-daemon.sock)
//!   LACS_LLM_PROVIDER — provider name (default: ollama)
//!   LACS_LLM_MODEL    — model override (default: provider's default)
//!   LACS_OLLAMA_URL   — Ollama base URL (default: http://localhost:11434)
//!
//! Exit codes:
//!   0 — plan produced successfully
//!   1 — planning failed (error printed to stderr)
//!   2 — usage / configuration error

use std::io::{self, Read};
use std::process;

use lacs_brain::config::BrainConfig;
use lacs_brain::planner::{LlmPlanner, PlanningError};
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
        stream
            .set_read_timeout(Some(Duration::from_secs(10)))
            .ok();
        stream
            .set_write_timeout(Some(Duration::from_secs(10)))
            .ok();

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
                .map_err(|e| PlanningError::StateUnavailable(e))
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
        stream
            .set_read_timeout(Some(Duration::from_secs(10)))
            .ok();
        stream
            .set_write_timeout(Some(Duration::from_secs(10)))
            .ok();

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
            Some("query_action_response") => {
                Ok(resp["output"].as_str().unwrap_or("").to_string())
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
    // Read intent from stdin.
    let mut intent = String::new();
    if let Err(e) = io::stdin().read_to_string(&mut intent) {
        eprintln!("lacs-test-cli: failed to read stdin: {e}");
        process::exit(2);
    }
    let intent = intent.trim();
    if intent.is_empty() {
        eprintln!("lacs-test-cli: no intent provided on stdin");
        eprintln!("usage: echo 'show me disk usage' | lacs-test-cli");
        process::exit(2);
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
            process::exit(2);
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
            process::exit(2);
        }
    };

    // Run the planning loop.
    eprintln!("lacs-test-cli: planning intent: {intent}");
    match planner.plan_intent(intent).await {
        Ok(plan) => {
            let json = serde_json::to_string_pretty(&plan).expect("Plan is always serialisable");
            println!("{json}");
            process::exit(0);
        }
        Err(PlanningError::EmptyIntent) => {
            eprintln!("lacs-test-cli: empty intent");
            process::exit(2);
        }
        Err(e) => {
            eprintln!("lacs-test-cli: planning failed: {e}");
            process::exit(1);
        }
    }
}
