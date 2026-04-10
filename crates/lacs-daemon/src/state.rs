use crate::transactions::{TransactionStore, TransactionStoreError};
use crate::transport::grpc::ListenTarget;
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
}

#[derive(Debug, thiserror::Error)]
pub enum DaemonStateError {
    #[error(transparent)]
    Transactions(#[from] TransactionStoreError),
}

impl DaemonState {
    pub fn open(config: DaemonConfig) -> Result<Self, DaemonStateError> {
        let transactions = TransactionStore::open(&config.database_path)?;
        Ok(Self {
            config,
            transactions,
        })
    }
}
