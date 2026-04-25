use std::collections::HashMap;
use std::sync::Arc;

use sysknife_core::{config::LacsConfig, default_database_path, default_listen_uri};
use sysknife_daemon::audit_forward::{self, AuditForwarder, AuditSinkSpec};
use sysknife_daemon::dispatcher::{connection_handler, resolve_caller_role};
use sysknife_daemon::policy::PolicyTable;
use sysknife_daemon::state::{DaemonConfig, DaemonState};
use sysknife_daemon::state_collector::RealCommandRunner;
use sysknife_daemon::transport::grpc::{bind_unix_listener, ListenTarget};
use tokio::net::UnixListener;
use tokio::sync::Semaphore;

/// Maximum number of concurrent IPC connections the daemon accepts.
///
/// Each shell instance opens one connection per plan step. 16 slots allow
/// 16 concurrent shell sessions before excess connections are dropped.
/// Raising this too high risks file descriptor exhaustion (EMFILE) under load.
const MAX_CONNECTIONS: usize = 16;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Apply config-file values as env var defaults before reading any config.
    // Must run before the tokio runtime starts worker threads.
    let lacs_config = LacsConfig::load();
    lacs_config.apply_defaults_to_env();

    let listen_uri = default_listen_uri();
    let database_path = default_database_path();

    // Build the policy table from `[policy.risk_overrides]` (if any). A
    // typo, an unknown action, or a downgrade attempt is a fatal startup
    // error — operators must see misconfiguration loudly, not silently.
    let raw_overrides: HashMap<String, String> = lacs_config
        .policy
        .as_ref()
        .and_then(|p| p.risk_overrides.clone())
        .unwrap_or_default();
    let policy = PolicyTable::from_overrides(&raw_overrides).map_err(|e| {
        eprintln!("[sysknife-daemon] FATAL: policy validation failed: {e}");
        e
    })?;

    if policy.override_count() > 0 {
        eprintln!(
            "[sysknife-daemon] applying {} risk override(s) from [policy.risk_overrides]:",
            policy.override_count()
        );
        for (action, role) in policy.active_overrides() {
            eprintln!("[sysknife-daemon]   {action:30} → {role:?}");
        }
    }

    // Optional external audit log forwarding (#150). Spawned before
    // DaemonState is constructed so the state can hold the handle.
    let forwarder: Option<AuditForwarder> = match build_forwarder(lacs_config.audit.as_ref()) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("[sysknife-daemon] FATAL: audit forwarder config invalid: {e}");
            return Err(e.into());
        }
    };
    if forwarder.is_some() {
        eprintln!("[sysknife-daemon] audit-forward: external sink active");
    }

    let listen_target = ListenTarget::try_from_uri(&listen_uri)?;
    let config = DaemonConfig::new(listen_target.clone(), &database_path);
    let state = DaemonState::open_full(config, policy, forwarder)?;

    let runner = Arc::new(RealCommandRunner);
    let semaphore = Arc::new(Semaphore::new(MAX_CONNECTIONS));

    eprintln!("[sysknife-daemon] listening on {listen_uri}");

    match listen_target {
        ListenTarget::Unix(path) => {
            let std_listener = bind_unix_listener(&ListenTarget::Unix(path))?;
            std_listener.set_nonblocking(true)?;
            let listener = UnixListener::from_std(std_listener)?;
            unix_accept_loop(listener, state, runner, semaphore).await;
        }
        #[cfg(target_os = "linux")]
        ListenTarget::Vsock { port } => {
            use sysknife_daemon::transport::grpc::bind_vsock_listener;
            let listener = bind_vsock_listener(port)?;
            vsock_accept_loop(listener, state, runner, semaphore).await;
        }
    }

    Ok(())
}

async fn unix_accept_loop(
    listener: UnixListener,
    state: sysknife_daemon::state::DaemonState,
    runner: Arc<RealCommandRunner>,
    semaphore: Arc<Semaphore>,
) {
    loop {
        tokio::select! {
            accept = listener.accept() => {
                match accept {
                    Ok((stream, _addr)) => {
                        match Arc::clone(&semaphore).try_acquire_owned() {
                            Ok(permit) => {
                                let role = resolve_caller_role(&stream);
                                let state = state.clone();
                                let runner = Arc::clone(&runner);
                                tokio::spawn(async move {
                                    connection_handler(stream, state, runner, role).await;
                                    drop(permit);
                                });
                            }
                            Err(_) => {
                                eprintln!(
                                    "[sysknife-daemon] connection limit ({MAX_CONNECTIONS}) reached; \
                                     dropping new connection"
                                );
                            }
                        }
                    }
                    Err(e) => {
                        use std::io::ErrorKind;
                        match e.kind() {
                            ErrorKind::ConnectionAborted | ErrorKind::ConnectionReset => {
                                eprintln!("[sysknife-daemon] transient accept error: {e}");
                            }
                            _ => {
                                eprintln!("[sysknife-daemon] fatal accept error, shutting down: {e}");
                                break;
                            }
                        }
                    }
                }
            }
            _ = tokio::signal::ctrl_c() => {
                eprintln!("[sysknife-daemon] shutting down");
                break;
            }
        }
    }
}

#[cfg(target_os = "linux")]
async fn vsock_accept_loop(
    listener: tokio_vsock::VsockListener,
    state: sysknife_daemon::state::DaemonState,
    runner: Arc<RealCommandRunner>,
    semaphore: Arc<Semaphore>,
) {
    use sysknife_daemon::dispatcher::vsock_connection_handler;

    loop {
        tokio::select! {
            accept = listener.accept() => {
                match accept {
                    Ok((stream, addr)) => {
                        eprintln!("[sysknife-daemon] vsock connection from cid={}", addr.cid());
                        match Arc::clone(&semaphore).try_acquire_owned() {
                            Ok(permit) => {
                                let state = state.clone();
                                let runner = Arc::clone(&runner);
                                tokio::spawn(async move {
                                    vsock_connection_handler(stream, state, runner).await;
                                    drop(permit);
                                });
                            }
                            Err(_) => {
                                eprintln!(
                                    "[sysknife-daemon] connection limit ({MAX_CONNECTIONS}) reached; \
                                     dropping vsock connection"
                                );
                            }
                        }
                    }
                    Err(e) => {
                        use std::io::ErrorKind;
                        match e.kind() {
                            ErrorKind::ConnectionAborted | ErrorKind::ConnectionReset => {
                                eprintln!("[sysknife-daemon] transient vsock accept error: {e}");
                            }
                            _ => {
                                eprintln!(
                                    "[sysknife-daemon] fatal vsock accept error, shutting down: {e}"
                                );
                                break;
                            }
                        }
                    }
                }
            }
            _ = tokio::signal::ctrl_c() => {
                eprintln!("[sysknife-daemon] shutting down");
                break;
            }
        }
    }
}

/// Build the audit forwarder from `[audit.forward]` config. Returns `None` if
/// no sinks are configured. Returns `Err` if a sink is enabled but its
/// configuration is invalid (e.g. unparseable host).
fn build_forwarder(
    audit: Option<&sysknife_core::config::AuditSection>,
) -> Result<Option<AuditForwarder>, std::io::Error> {
    let Some(audit) = audit else {
        return Ok(None);
    };
    let Some(forward) = audit.forward.as_ref() else {
        return Ok(None);
    };
    let Some(syslog) = forward.syslog.as_ref() else {
        return Ok(None);
    };
    let host: std::net::SocketAddr = syslog.host.parse().map_err(|e| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!(
                "[audit.forward.syslog] host {:?} is not a valid host:port: {e}",
                syslog.host
            ),
        )
    })?;
    Ok(Some(audit_forward::spawn(AuditSinkSpec::SyslogUdp {
        host,
        facility: syslog.facility,
    })))
}

#[cfg(test)]
mod tests {
    #[test]
    fn max_connections_is_reasonable() {
        assert!(
            super::MAX_CONNECTIONS >= 4,
            "MAX_CONNECTIONS {} too low; need at least one connection per shell + headroom",
            super::MAX_CONNECTIONS
        );
        assert!(
            super::MAX_CONNECTIONS <= 64,
            "MAX_CONNECTIONS {} too high; each connection holds DB state",
            super::MAX_CONNECTIONS
        );
    }
}
