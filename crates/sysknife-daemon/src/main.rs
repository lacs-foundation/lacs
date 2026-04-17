use std::sync::Arc;

use sysknife_core::{
    config::LacsConfig,
    {DEFAULT_DATABASE_PATH, DEFAULT_LISTEN_URI},
};
use sysknife_daemon::dispatcher::{connection_handler, resolve_caller_role};
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
    LacsConfig::load().apply_defaults_to_env();

    let listen_uri =
        std::env::var("SYSKNIFE_LISTEN_URI").unwrap_or_else(|_| DEFAULT_LISTEN_URI.to_string());
    let database_path = std::env::var("SYSKNIFE_DATABASE_PATH")
        .unwrap_or_else(|_| DEFAULT_DATABASE_PATH.to_string());

    let listen_target = ListenTarget::try_from_uri(&listen_uri)?;
    let config = DaemonConfig::new(listen_target.clone(), &database_path);
    let state = DaemonState::open(config)?;

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
