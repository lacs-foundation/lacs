use std::sync::Arc;

use lacs_core::{DEFAULT_DATABASE_PATH, DEFAULT_LISTEN_URI};
use lacs_daemon::dispatcher::{connection_handler, resolve_caller_role};
use lacs_daemon::state::{DaemonConfig, DaemonState};
use lacs_daemon::state_collector::RealCommandRunner;
use lacs_daemon::transport::grpc::ListenTarget;
use tokio::net::UnixListener;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let listen_uri =
        std::env::var("LACS_LISTEN_URI").unwrap_or_else(|_| DEFAULT_LISTEN_URI.to_string());
    let database_path =
        std::env::var("LACS_DATABASE_PATH").unwrap_or_else(|_| DEFAULT_DATABASE_PATH.to_string());

    let listen_target = ListenTarget::try_from_uri(&listen_uri)?;
    let config = DaemonConfig::new(listen_target, &database_path);
    let runtime = DaemonState::bootstrap(config)?;

    // Convert the std UnixListener from bootstrap into a tokio UnixListener.
    let std_listener = runtime.listener;
    std_listener.set_nonblocking(true)?;
    let listener = UnixListener::from_std(std_listener)?;

    let runner = Arc::new(RealCommandRunner);
    let state = runtime.state;

    eprintln!(
        "[lacs-daemon] listening on {}",
        std::env::var("LACS_LISTEN_URI").unwrap_or_else(|_| DEFAULT_LISTEN_URI.to_string())
    );

    loop {
        tokio::select! {
            accept = listener.accept() => {
                match accept {
                    Ok((stream, _addr)) => {
                        let role = resolve_caller_role(&stream);
                        let state = state.clone();
                        let runner = Arc::clone(&runner);
                        tokio::spawn(async move {
                            connection_handler(stream, state, runner, role).await;
                        });
                    }
                    Err(e) => {
                        eprintln!("[lacs-daemon] accept error: {e}");
                    }
                }
            }
            _ = tokio::signal::ctrl_c() => {
                eprintln!("[lacs-daemon] shutting down");
                break;
            }
        }
    }

    Ok(())
}
