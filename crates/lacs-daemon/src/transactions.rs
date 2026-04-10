use lacs_types::{JobState, RiskLevel, TransactionRecord};
use rusqlite::{params, Connection};
use serde::{de::DeserializeOwned, Serialize};
use std::path::{Path, PathBuf};
use uuid::Uuid;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NewTransaction {
    pub request_id: String,
    pub request_hash: String,
    pub action_name: String,
    pub risk_level: RiskLevel,
    pub status: JobState,
    pub approval_id: Option<String>,
    pub summary: String,
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct TransactionStore {
    path: PathBuf,
}

#[derive(Debug, thiserror::Error)]
pub enum TransactionStoreError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),

    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
}

impl TransactionStore {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, TransactionStoreError> {
        let path = path.as_ref().to_path_buf();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let store = Self { path };
        store.initialize()?;
        Ok(store)
    }

    pub fn record(
        &self,
        transaction: NewTransaction,
    ) -> Result<TransactionRecord, TransactionStoreError> {
        let transaction_id = Uuid::new_v4().to_string();
        let request_id = transaction.request_id;
        let request_hash = transaction.request_hash;
        let action_name = transaction.action_name;
        let risk_level = transaction.risk_level;
        let status = transaction.status;
        let approval_id = transaction.approval_id;
        let summary = transaction.summary;
        let warnings = transaction.warnings;

        let record = TransactionRecord {
            transaction_id: transaction_id.clone(),
            request_id: request_id.clone(),
            request_hash: request_hash.clone(),
            action_name: action_name.clone(),
            risk_level: risk_level.clone(),
            status: status.clone(),
            approval_id: approval_id.clone(),
            summary: summary.clone(),
            warnings: warnings.clone(),
        };

        let conn = self.connection()?;
        conn.execute(
            "INSERT INTO transactions (
                transaction_id,
                request_id,
                request_hash,
                action_name,
                risk_level,
                status,
                approval_id,
                summary,
                warnings_json
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                transaction_id,
                request_id,
                request_hash,
                action_name,
                serialize_field(&risk_level)?,
                serialize_field(&status)?,
                approval_id,
                summary,
                serde_json::to_string(&warnings)?,
            ],
        )?;
        Ok(record)
    }

    pub fn get(
        &self,
        transaction_id: &str,
    ) -> Result<Option<TransactionRecord>, TransactionStoreError> {
        let conn = self.connection()?;
        let mut stmt = conn.prepare(
            "SELECT
                transaction_id,
                request_id,
                request_hash,
                action_name,
                risk_level,
                status,
                approval_id,
                summary,
                warnings_json
             FROM transactions
             WHERE transaction_id = ?1",
        )?;
        let mut rows = stmt.query(params![transaction_id])?;
        if let Some(row) = rows.next()? {
            Ok(Some(TransactionRecord {
                transaction_id: row.get(0)?,
                request_id: row.get(1)?,
                request_hash: row.get(2)?,
                action_name: row.get(3)?,
                risk_level: deserialize_field(&row.get::<_, String>(4)?)?,
                status: deserialize_field(&row.get::<_, String>(5)?)?,
                approval_id: row.get(6)?,
                summary: row.get(7)?,
                warnings: serde_json::from_str(&row.get::<_, String>(8)?)?,
            }))
        } else {
            Ok(None)
        }
    }

    fn connection(&self) -> Result<Connection, TransactionStoreError> {
        Ok(Connection::open(&self.path)?)
    }

    fn initialize(&self) -> Result<(), TransactionStoreError> {
        let conn = self.connection()?;
        conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS transactions (
                transaction_id TEXT PRIMARY KEY,
                request_id TEXT NOT NULL,
                request_hash TEXT NOT NULL,
                action_name TEXT NOT NULL,
                risk_level TEXT NOT NULL,
                status TEXT NOT NULL,
                approval_id TEXT,
                summary TEXT NOT NULL,
                warnings_json TEXT NOT NULL
            );
            "#,
        )?;
        Ok(())
    }
}

fn serialize_field<T: Serialize>(value: &T) -> Result<String, serde_json::Error> {
    serde_json::to_string(value)
}

fn deserialize_field<T: DeserializeOwned>(value: &str) -> Result<T, serde_json::Error> {
    serde_json::from_str(value)
}
