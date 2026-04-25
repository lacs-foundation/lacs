use crate::policy::PolicyTable;
use crate::transactions::{TransactionStore, TransactionStoreError};
use crate::transport::grpc::{bind_unix_listener, ListenTarget, ListenTargetError};
use std::os::unix::net::UnixListener;
use std::path::PathBuf;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DaemonConfig {
    pub listen_target: ListenTarget,
    pub database_path: PathBuf,
}

impl DaemonConfig {
    pub fn new(listen_target: ListenTarget, database_path: impl Into<PathBuf>) -> Self {
        Self {
            listen_target,
            database_path: database_path.into(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct DaemonState {
    pub config: DaemonConfig,
    pub transactions: TransactionStore,
    pub policy: PolicyTable,
}

#[derive(Debug)]
pub struct DaemonRuntime {
    pub state: DaemonState,
    pub listener: UnixListener,
}

#[derive(Debug, thiserror::Error)]
pub enum DaemonStateError {
    #[error(transparent)]
    Transactions(#[from] TransactionStoreError),

    #[error(transparent)]
    Listen(#[from] ListenTargetError),
}

impl DaemonState {
    /// Open the daemon state with no policy overrides (compile-time baseline).
    /// Suitable for tests and dev runs.
    pub fn open(config: DaemonConfig) -> Result<Self, DaemonStateError> {
        Self::open_with_policy(config, PolicyTable::empty())
    }

    /// Open the daemon state with an explicit policy table.
    /// Production callers (`main.rs`) build the table from
    /// `[policy.risk_overrides]` in `config.toml`.
    pub fn open_with_policy(
        config: DaemonConfig,
        policy: PolicyTable,
    ) -> Result<Self, DaemonStateError> {
        let transactions = TransactionStore::open(&config.database_path)?;
        Ok(Self {
            config,
            transactions,
            policy,
        })
    }

    pub fn bootstrap(config: DaemonConfig) -> Result<DaemonRuntime, DaemonStateError> {
        let state = Self::open(config)?;
        let listener = bind_unix_listener(&state.config.listen_target)?;
        Ok(DaemonRuntime { state, listener })
    }
}
