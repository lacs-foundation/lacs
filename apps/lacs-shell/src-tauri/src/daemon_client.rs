//! Synchronous IPC client for querying the daemon's state.
//!
//! `DaemonIpcClient` implements [`StateClient`] using a per-call Unix domain
//! socket connection with the daemon's length-prefixed JSON framing protocol.
//!
//! The synchronous interface matches `StateClient::curated_state()`, which the
//! planner invokes while holding no async resources. The brief blocking read is
//! acceptable inside a multi-threaded tokio runtime (Tauri's default).
//!
//! # Framing protocol
//!
//! Every message is prefixed by a 4-byte little-endian `u32` that gives the
//! payload length in bytes, followed by the UTF-8 JSON payload. This mirrors
//! the daemon's `FramedStream` exactly.

use std::io::{self, Read, Write};
use std::os::unix::net::UnixStream;

use lacs_brain::planner::PlanningError;
use lacs_brain::state_client::{CuratedState, StateClient};
use serde_json::Value;

/// Maximum response size accepted from the daemon (4 MiB — mirrors daemon limit).
const MAX_RESPONSE_BYTES: u32 = 4 * 1024 * 1024;

/// A [`StateClient`] that queries a running `lacs-daemon` over its Unix socket.
///
/// Opens a fresh connection per call. Suitable for the LLM planning loop where
/// calls are infrequent and persistent connection management would add
/// unnecessary complexity.
pub struct DaemonIpcClient {
    socket_path: String,
}

impl DaemonIpcClient {
    /// Create a client that connects to `socket_path`.
    ///
    /// The path should be the filesystem path portion of the daemon's listen
    /// URI, e.g. `"/tmp/lacs-daemon.sock"`.
    pub fn new(socket_path: impl Into<String>) -> Self {
        Self {
            socket_path: socket_path.into(),
        }
    }

    fn query_state_inner(&self) -> Result<CuratedState, String> {
        let mut stream = UnixStream::connect(&self.socket_path)
            .map_err(|e| format!("cannot connect to daemon at {}: {e}", self.socket_path))?;

        let request =
            serde_json::to_vec(&serde_json::json!({
                "type": "query_state",
                "request_id": "shell-state-query"
            }))
            .expect("static JSON is always serialisable");

        write_framed(&mut stream, &request)
            .map_err(|e| format!("failed to send query_state: {e}"))?;

        let msg = read_framed(&mut stream)
            .map_err(|e| format!("failed to read daemon response: {e}"))?;

        let resp: Value = serde_json::from_slice(&msg)
            .map_err(|e| format!("invalid JSON from daemon: {e}"))?;

        match resp["type"].as_str() {
            Some("state_response") => {
                let s = &resp["state"];
                Ok(CuratedState {
                    host_name: s["host_name"].as_str().unwrap_or("").to_string(),
                    deployment: s["deployment"].as_str().unwrap_or("").to_string(),
                    services: string_array(&s["services"]),
                    flatpaks: string_array(&s["flatpaks"]),
                    toolboxes: string_array(&s["toolboxes"]),
                })
            }
            Some("error_response") => Err(format!(
                "daemon error ({}): {}",
                resp["category"].as_str().unwrap_or("unknown"),
                resp["message"].as_str().unwrap_or("no message")
            )),
            other => Err(format!(
                "unexpected response type from daemon: {}",
                other.unwrap_or("<missing>")
            )),
        }
    }
}

impl StateClient for DaemonIpcClient {
    fn curated_state(&self) -> Result<CuratedState, PlanningError> {
        self.query_state_inner()
            .map_err(PlanningError::StateUnavailable)
    }
}

// ---------------------------------------------------------------------------
// Framing helpers (mirrors lacs-daemon's FramedStream protocol)
// ---------------------------------------------------------------------------

fn write_framed(stream: &mut UnixStream, msg: &[u8]) -> io::Result<()> {
    let len = u32::try_from(msg.len()).map_err(|_| {
        io::Error::new(io::ErrorKind::InvalidInput, "message exceeds 4 GiB limit")
    })?;
    stream.write_all(&len.to_le_bytes())?;
    stream.write_all(msg)
}

fn read_framed(stream: &mut UnixStream) -> io::Result<Vec<u8>> {
    let mut len_buf = [0u8; 4];
    stream.read_exact(&mut len_buf)?;
    let len = u32::from_le_bytes(len_buf);
    if len > MAX_RESPONSE_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("daemon response too large: {len} bytes"),
        ));
    }
    let mut msg = vec![0u8; len as usize];
    stream.read_exact(&mut msg)?;
    Ok(msg)
}

fn string_array(v: &Value) -> Vec<String> {
    v.as_array()
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use lacs_brain::planner::PlanningError;
    use std::io::{Read, Write};
    use std::os::unix::net::UnixListener;
    use tempfile::tempdir;

    /// Spawn a mock daemon that accepts one connection, discards the request,
    /// and writes back `response`.
    fn mock_daemon(socket_path: &std::path::Path, response: serde_json::Value) {
        let listener = UnixListener::bind(socket_path).unwrap();
        std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();

            // Read and discard the request frame.
            let mut len_buf = [0u8; 4];
            if stream.read_exact(&mut len_buf).is_err() {
                return;
            }
            let len = u32::from_le_bytes(len_buf) as usize;
            let mut buf = vec![0u8; len];
            let _ = stream.read_exact(&mut buf);

            // Write back the mocked response.
            let resp_bytes = serde_json::to_vec(&response).unwrap();
            let resp_len = resp_bytes.len() as u32;
            let _ = stream.write_all(&resp_len.to_le_bytes());
            let _ = stream.write_all(&resp_bytes);
        });
    }

    #[test]
    fn curated_state_parses_state_response() {
        let dir = tempdir().unwrap();
        let socket_path = dir.path().join("daemon.sock");

        mock_daemon(
            &socket_path,
            serde_json::json!({
                "type": "state_response",
                "request_id": "shell-state-query",
                "state": {
                    "host_name": "silverblue-test",
                    "deployment": r#"{"deployments":[]}"#,
                    "services": ["NetworkManager.service"],
                    "flatpaks": ["org.mozilla.firefox"],
                    "toolboxes": ["lacs-dev"]
                }
            }),
        );

        // Give the thread time to bind.
        std::thread::sleep(std::time::Duration::from_millis(10));

        let client = DaemonIpcClient::new(socket_path.to_str().unwrap());
        let state = client.curated_state().unwrap();

        assert_eq!(state.host_name, "silverblue-test");
        assert_eq!(state.deployment, r#"{"deployments":[]}"#);
        assert_eq!(state.services, vec!["NetworkManager.service"]);
        assert_eq!(state.flatpaks, vec!["org.mozilla.firefox"]);
        assert_eq!(state.toolboxes, vec!["lacs-dev"]);
    }

    #[test]
    fn curated_state_maps_error_response_to_state_unavailable() {
        let dir = tempdir().unwrap();
        let socket_path = dir.path().join("daemon.sock");

        mock_daemon(
            &socket_path,
            serde_json::json!({
                "type": "error_response",
                "request_id": "shell-state-query",
                "category": "state_collection_failed",
                "message": "rpm-ostree timed out"
            }),
        );

        std::thread::sleep(std::time::Duration::from_millis(10));

        let client = DaemonIpcClient::new(socket_path.to_str().unwrap());
        let err = client.curated_state().unwrap_err();
        assert!(
            matches!(&err, PlanningError::StateUnavailable(s) if s.contains("state_collection_failed")),
            "expected StateUnavailable with category, got: {err:?}"
        );
    }

    #[test]
    fn curated_state_fails_when_daemon_unreachable() {
        let client = DaemonIpcClient::new("/tmp/lacs-daemon-test-not-running.sock");
        let err = client.curated_state().unwrap_err();
        assert!(
            matches!(err, PlanningError::StateUnavailable(_)),
            "expected StateUnavailable on connection failure, got: {err:?}"
        );
    }
}
