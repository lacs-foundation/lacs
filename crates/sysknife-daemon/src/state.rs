use crate::audit_forward::AuditForwarder;
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
    /// Optional external audit-log forwarder. `None` when no `[audit.forward]`
    /// sink is configured; events recorded by the dispatcher are then only
    /// written to the local hash-chained store.
    pub forwarder: Option<AuditForwarder>,
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
    /// Open the daemon state with no policy overrides and no forwarding.
    /// Suitable for tests and dev runs.
    pub fn open(config: DaemonConfig) -> Result<Self, DaemonStateError> {
        Self::open_with_policy(config, PolicyTable::empty())
    }

    /// Open the daemon state with an explicit policy table and no forwarding.
    pub fn open_with_policy(
        config: DaemonConfig,
        policy: PolicyTable,
    ) -> Result<Self, DaemonStateError> {
        Self::open_full(config, policy, None)
    }

    /// Open the daemon state with full configuration. Production callers
    /// (`main.rs`) build the policy table from `[policy.risk_overrides]` and
    /// the forwarder from `[audit.forward]`.
    pub fn open_full(
        config: DaemonConfig,
        policy: PolicyTable,
        forwarder: Option<AuditForwarder>,
    ) -> Result<Self, DaemonStateError> {
        let transactions = TransactionStore::open(&config.database_path)?;
        Ok(Self {
            config,
            transactions,
            policy,
            forwarder,
        })
    }

    pub fn bootstrap(config: DaemonConfig) -> Result<DaemonRuntime, DaemonStateError> {
        let state = Self::open(config)?;
        let listener = bind_unix_listener(&state.config.listen_target)?;
        Ok(DaemonRuntime { state, listener })
    }
}
