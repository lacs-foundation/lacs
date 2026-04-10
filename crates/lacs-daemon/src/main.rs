use lacs_core::{DEFAULT_DATABASE_PATH, DEFAULT_LISTEN_URI};
use lacs_daemon::state::DaemonConfig;
use lacs_daemon::state::DaemonState;
use lacs_daemon::transport::grpc::ListenTarget;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let listen_uri =
        std::env::var("LACS_LISTEN_URI").unwrap_or_else(|_| DEFAULT_LISTEN_URI.to_string());
    let database_path =
        std::env::var("LACS_DATABASE_PATH").unwrap_or_else(|_| DEFAULT_DATABASE_PATH.to_string());
    let listen_target = ListenTarget::try_from_uri(&listen_uri)?;
    let config = DaemonConfig::new(listen_target, database_path);
    let _runtime = DaemonState::bootstrap(config)?;

    tokio::signal::ctrl_c().await?;
    Ok(())
}
