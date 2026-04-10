use lacs_daemon::state::DaemonConfig;
use lacs_daemon::state::DaemonState;
use lacs_daemon::transport::grpc::ListenTarget;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let listen_uri = std::env::var("LACS_LISTEN_URI")
        .unwrap_or_else(|_| "unix:///tmp/lacs-daemon.sock".to_string());
    let database_path = std::env::var("LACS_DATABASE_PATH")
        .unwrap_or_else(|_| "/tmp/lacs-daemon.sqlite".to_string());
    let listen_target = ListenTarget::try_from_uri(&listen_uri)?;
    let config = DaemonConfig::new(listen_target, database_path);
    let _runtime = DaemonState::bootstrap(config)?;

    tokio::signal::ctrl_c().await?;
    Ok(())
}
