use lacs_types::{JobState, PreviewEnvelope, RiskLevel, TransactionRecord};
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

#[derive(Clone, Debug, PartialEq)]
pub struct RecordedPreviewedTransaction {
    pub transaction: TransactionRecord,
    pub preview: PreviewEnvelope,
}

#[derive(Debug, thiserror::Error)]
pub enum TransactionStoreError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),

    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("transaction not found: {0}")]
    NotFound(String),

    #[error("invalid transition from {from:?} to {to:?}")]
    InvalidTransition { from: JobState, to: JobState },
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
        let conn = self.connection()?;
        let transaction_id = Uuid::new_v4().to_string();
        Self::insert_transaction(&conn, &transaction_id, transaction)
    }

    pub fn record_previewed(
        &self,
        transaction: NewTransaction,
        preview: PreviewEnvelope,
    ) -> Result<RecordedPreviewedTransaction, TransactionStoreError> {
        let mut conn = self.connection()?;
        let tx = conn.transaction()?;
        let transaction_id = Uuid::new_v4().to_string();
        let transaction = Self::insert_transaction(&tx, &transaction_id, transaction)?;
        Self::insert_preview(&tx, &transaction.transaction_id, &preview)?;
        tx.commit()?;

        Ok(RecordedPreviewedTransaction {
            transaction,
            preview,
        })
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
            Ok(Some(row_to_record(row)?))
        } else {
            Ok(None)
        }
    }

    /// Find the most-recently recorded transaction with the given `request_hash`
    /// that is still in `Queued` status.
    ///
    /// Returns `None` if no matching Queued transaction exists. The dispatcher
    /// uses this to enforce preview-before-execute and to block replay attacks:
    /// an already-executed (Succeeded/Failed) transaction is never returned,
    /// so a captured approval hash cannot be reused after the first execute.
    pub fn find_by_request_hash(
        &self,
        request_hash: &str,
    ) -> Result<Option<TransactionRecord>, TransactionStoreError> {
        let conn = self.connection()?;
        // Status is stored as its JSON serialization (e.g. `"queued"`).
        let queued_json = serialize_field(&JobState::Queued)?;
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
             WHERE request_hash = ?1
               AND status = ?2
               AND created_at > datetime('now', '-15 minutes')
             ORDER BY rowid DESC
             LIMIT 1",
        )?;
        let mut rows = stmt.query(params![request_hash, queued_json])?;
        if let Some(row) = rows.next()? {
            Ok(Some(row_to_record(row)?))
        } else {
            Ok(None)
        }
    }

    pub fn get_preview(
        &self,
        transaction_id: &str,
    ) -> Result<Option<PreviewEnvelope>, TransactionStoreError> {
        let conn = self.connection()?;
        let mut stmt = conn.prepare(
            "SELECT preview_json
             FROM transaction_previews
             WHERE transaction_id = ?1",
        )?;
        let mut rows = stmt.query(params![transaction_id])?;
        if let Some(row) = rows.next()? {
            let preview_json: String = row.get(0)?;
            Ok(Some(serde_json::from_str(&preview_json)?))
        } else {
            Ok(None)
        }
    }

    pub fn update_status(
        &self,
        transaction_id: &str,
        new_status: JobState,
    ) -> Result<(), TransactionStoreError> {
        let conn = self.connection()?;

        // Read the current status so we can validate the transition.
        let current_status: String = conn
            .query_row(
                "SELECT status FROM transactions WHERE transaction_id = ?1",
                params![transaction_id],
                |row| row.get(0),
            )
            .map_err(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => {
                    TransactionStoreError::NotFound(transaction_id.to_string())
                }
                other => TransactionStoreError::Sqlite(other),
            })?;

        let current: JobState = deserialize_field(&current_status)?;
        if !crate::jobs::allowed_transition(&current, &new_status) {
            return Err(TransactionStoreError::InvalidTransition {
                from: current,
                to: new_status,
            });
        }

        conn.execute(
            "UPDATE transactions SET status = ?1 WHERE transaction_id = ?2",
            params![serialize_field(&new_status)?, transaction_id],
        )?;
        Ok(())
    }

    /// Atomically claim a `Queued` transaction for execution by transitioning
    /// its status to `Running` in a single SQL statement.
    ///
    /// Returns `Ok(true)` if the transaction was claimed (it was in `Queued`
    /// state). Returns `Ok(false)` if the transaction could not be claimed —
    /// it either does not exist or was already transitioned away from `Queued`
    /// by a concurrent request.
    ///
    /// This closes the TOCTOU window in replay protection: two concurrent
    /// execute requests that both pass `find_by_request_hash` cannot both
    /// proceed — only the first `claim_for_execution` wins; the loser must
    /// return `stale_approval`.
    pub fn claim_for_execution(&self, transaction_id: &str) -> Result<bool, TransactionStoreError> {
        let conn = self.connection()?;
        let queued_json = serialize_field(&JobState::Queued)?;
        let running_json = serialize_field(&JobState::Running)?;
        let rows_affected = conn.execute(
            "UPDATE transactions SET status = ?1 \
             WHERE transaction_id = ?2 AND status = ?3",
            params![running_json, transaction_id, queued_json],
        )?;
        Ok(rows_affected > 0)
    }

    /// Cancel all `Queued` transactions whose `created_at` timestamp is older
    /// than the 15-minute TTL window.  Returns the number of rows affected.
    pub fn cleanup_stale_queued(&self) -> Result<u64, TransactionStoreError> {
        let conn = self.connection()?;
        let canceled_json = serialize_field(&JobState::Canceled)?;
        let queued_json = serialize_field(&JobState::Queued)?;
        let rows_affected = conn.execute(
            "UPDATE transactions SET status = ?1 \
             WHERE status = ?2 \
               AND created_at <= datetime('now', '-15 minutes')",
            params![canceled_json, queued_json],
        )?;
        Ok(rows_affected as u64)
    }

    /// List transactions with optional filters, ordered by newest first.
    ///
    /// - `limit`: max number of rows (capped at 100)
    /// - `status_filter`: if set, only return rows matching this status
    ///   (must be a valid `JobState` variant: `"succeeded"`, `"failed"`,
    ///   `"queued"`, `"running"`, `"canceled"`, `"rolled_back"`, `"needs_reboot"`)
    /// - `action_filter`: if set, only return rows with this exact action name
    /// - `since_hours`: if set, only return rows created within the last N hours
    pub fn list_transactions(
        &self,
        limit: u32,
        status_filter: Option<&str>,
        action_filter: Option<&str>,
        since_hours: Option<u32>,
    ) -> Result<Vec<TransactionRecord>, TransactionStoreError> {
        let conn = self.connection()?;
        let limit = limit.min(100);

        let mut sql = String::from(
            "SELECT transaction_id, request_id, request_hash, action_name, \
             risk_level, status, approval_id, summary, warnings_json \
             FROM transactions WHERE 1=1",
        );
        let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

        if let Some(status) = status_filter {
            // Validate against known JobState variants to avoid silent empty
            // results from typos (e.g. "success" instead of "succeeded").
            // deserialize_field returns serde_json::Error → TransactionStoreError::Json.
            let validated: JobState = deserialize_field(&format!("\"{status}\""))?;
            let status_json = serialize_field(&validated)?;
            sql.push_str(" AND status = ?");
            param_values.push(Box::new(status_json));
        }

        if let Some(action) = action_filter {
            sql.push_str(" AND action_name = ?");
            param_values.push(Box::new(action.to_string()));
        }

        if let Some(hours) = since_hours {
            sql.push_str(" AND created_at > datetime('now', '-' || ? || ' hours')");
            param_values.push(Box::new(hours));
        }

        sql.push_str(" ORDER BY rowid DESC LIMIT ?");
        param_values.push(Box::new(limit));

        let params_ref: Vec<&dyn rusqlite::types::ToSql> =
            param_values.iter().map(|b| b.as_ref()).collect();

        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(params_ref.as_slice(), |row| Ok(row_to_record(row)))?;

        let mut results = Vec::new();
        for row in rows {
            results.push(row??);
        }
        Ok(results)
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
                warnings_json TEXT NOT NULL,
                created_at TEXT NOT NULL DEFAULT (datetime('now'))
            );

            CREATE TABLE IF NOT EXISTS transaction_previews (
                transaction_id TEXT PRIMARY KEY,
                preview_json TEXT NOT NULL
            );
            "#,
        )?;
        Ok(())
    }

    fn insert_transaction(
        conn: &Connection,
        transaction_id: &str,
        transaction: NewTransaction,
    ) -> Result<TransactionRecord, TransactionStoreError> {
        let request_id = transaction.request_id;
        let request_hash = transaction.request_hash;
        let action_name = transaction.action_name;
        let risk_level = transaction.risk_level;
        let status = transaction.status;
        let approval_id = transaction.approval_id;
        let summary = transaction.summary;
        let warnings = transaction.warnings;

        let record = TransactionRecord {
            transaction_id: transaction_id.to_string(),
            request_id: request_id.clone(),
            request_hash: request_hash.clone(),
            action_name: action_name.clone(),
            risk_level,
            status,
            approval_id: approval_id.clone(),
            summary: summary.clone(),
            warnings: warnings.clone(),
        };

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

    fn insert_preview(
        conn: &Connection,
        transaction_id: &str,
        preview: &PreviewEnvelope,
    ) -> Result<(), TransactionStoreError> {
        conn.execute(
            "INSERT INTO transaction_previews (transaction_id, preview_json)
             VALUES (?1, ?2)",
            params![transaction_id, serde_json::to_string(preview)?],
        )?;
        Ok(())
    }
}

fn row_to_record(row: &rusqlite::Row) -> Result<TransactionRecord, TransactionStoreError> {
    Ok(TransactionRecord {
        transaction_id: row.get(0)?,
        request_id: row.get(1)?,
        request_hash: row.get(2)?,
        action_name: row.get(3)?,
        risk_level: deserialize_field(&row.get::<_, String>(4)?)?,
        status: deserialize_field(&row.get::<_, String>(5)?)?,
        approval_id: row.get(6)?,
        summary: row.get(7)?,
        warnings: serde_json::from_str(&row.get::<_, String>(8)?)?,
    })
}

fn serialize_field<T: Serialize>(value: &T) -> Result<String, serde_json::Error> {
    serde_json::to_string(value)
}

fn deserialize_field<T: DeserializeOwned>(value: &str) -> Result<T, serde_json::Error> {
    serde_json::from_str(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn queued_transaction() -> NewTransaction {
        NewTransaction {
            request_id: "req-1".to_string(),
            request_hash: "hash-abc".to_string(),
            action_name: "UpdateSystem".to_string(),
            risk_level: RiskLevel::High,
            status: JobState::Queued,
            approval_id: None,
            summary: "Upgrade the system".to_string(),
            warnings: vec![],
        }
    }

    #[test]
    fn update_status_transitions_queued_to_running() {
        let dir = tempdir().unwrap();
        let store = TransactionStore::open(dir.path().join("tx.db")).unwrap();
        let tx = store.record(queued_transaction()).unwrap();

        store
            .update_status(&tx.transaction_id, JobState::Running)
            .unwrap();

        let updated = store.get(&tx.transaction_id).unwrap().unwrap();
        assert_eq!(updated.status, JobState::Running);
    }

    #[test]
    fn update_status_transitions_running_to_succeeded() {
        let dir = tempdir().unwrap();
        let store = TransactionStore::open(dir.path().join("tx.db")).unwrap();
        let tx = store.record(queued_transaction()).unwrap();

        store
            .update_status(&tx.transaction_id, JobState::Running)
            .unwrap();
        store
            .update_status(&tx.transaction_id, JobState::Succeeded)
            .unwrap();

        let updated = store.get(&tx.transaction_id).unwrap().unwrap();
        assert_eq!(updated.status, JobState::Succeeded);
    }

    #[test]
    fn update_status_for_unknown_id_returns_not_found() {
        let dir = tempdir().unwrap();
        let store = TransactionStore::open(dir.path().join("tx.db")).unwrap();

        let result = store.update_status("does-not-exist", JobState::Running);
        assert!(
            matches!(result, Err(TransactionStoreError::NotFound(ref id)) if id == "does-not-exist"),
            "expected NotFound, got: {result:?}"
        );
    }

    #[test]
    fn update_status_leaves_other_fields_intact() {
        let dir = tempdir().unwrap();
        let store = TransactionStore::open(dir.path().join("tx.db")).unwrap();
        let tx = store.record(queued_transaction()).unwrap();

        store
            .update_status(&tx.transaction_id, JobState::Running)
            .unwrap();
        store
            .update_status(&tx.transaction_id, JobState::Failed)
            .unwrap();

        let updated = store.get(&tx.transaction_id).unwrap().unwrap();
        assert_eq!(updated.action_name, "UpdateSystem");
        assert_eq!(updated.risk_level, RiskLevel::High);
        assert_eq!(updated.status, JobState::Failed);
    }

    #[test]
    fn find_by_request_hash_returns_queued_transaction() {
        let dir = tempdir().unwrap();
        let store = TransactionStore::open(dir.path().join("tx.db")).unwrap();
        let tx = store.record(queued_transaction()).unwrap();

        let found = store.find_by_request_hash("hash-abc").unwrap();
        assert!(found.is_some());
        assert_eq!(found.unwrap().transaction_id, tx.transaction_id);
    }

    #[test]
    fn find_by_request_hash_returns_none_for_unknown_hash() {
        let dir = tempdir().unwrap();
        let store = TransactionStore::open(dir.path().join("tx.db")).unwrap();

        let found = store.find_by_request_hash("nonexistent-hash").unwrap();
        assert!(found.is_none());
    }

    #[test]
    fn find_by_request_hash_returns_none_after_transaction_executed() {
        // A transaction that has already been executed (Succeeded/Failed) must
        // not be returned — this blocks replay attacks where a captured approval
        // hash is submitted a second time after the first execute completes.
        let dir = tempdir().unwrap();
        let store = TransactionStore::open(dir.path().join("tx.db")).unwrap();
        let tx = store.record(queued_transaction()).unwrap();

        // Simulate completed execution (must go through Running first).
        store
            .update_status(&tx.transaction_id, JobState::Running)
            .unwrap();
        store
            .update_status(&tx.transaction_id, JobState::Succeeded)
            .unwrap();

        let found = store.find_by_request_hash("hash-abc").unwrap();
        assert!(
            found.is_none(),
            "executed transaction must not be returned (replay protection)"
        );
    }

    #[test]
    fn claim_for_execution_succeeds_for_queued_transaction() {
        let dir = tempdir().unwrap();
        let store = TransactionStore::open(dir.path().join("tx.db")).unwrap();
        let tx = store.record(queued_transaction()).unwrap();

        let claimed = store.claim_for_execution(&tx.transaction_id).unwrap();
        assert!(claimed, "should claim Queued transaction");

        let updated = store.get(&tx.transaction_id).unwrap().unwrap();
        assert_eq!(
            updated.status,
            JobState::Running,
            "status must be Running after claim"
        );
    }

    #[test]
    fn claim_for_execution_returns_false_when_already_running() {
        let dir = tempdir().unwrap();
        let store = TransactionStore::open(dir.path().join("tx.db")).unwrap();
        let tx = store.record(queued_transaction()).unwrap();

        assert!(
            store.claim_for_execution(&tx.transaction_id).unwrap(),
            "first claim must succeed"
        );
        assert!(
            !store.claim_for_execution(&tx.transaction_id).unwrap(),
            "second claim must return false — simulates concurrent execute request"
        );
    }

    #[test]
    fn claim_for_execution_returns_false_for_unknown_id() {
        let dir = tempdir().unwrap();
        let store = TransactionStore::open(dir.path().join("tx.db")).unwrap();

        let claimed = store.claim_for_execution("does-not-exist").unwrap();
        assert!(!claimed, "unknown transaction must not be claimable");
    }

    #[test]
    fn find_by_request_hash_returns_none_for_running_transaction() {
        // A Running transaction must not be returned — it has already been
        // claimed by a concurrent request and must not be executed again.
        let dir = tempdir().unwrap();
        let store = TransactionStore::open(dir.path().join("tx.db")).unwrap();
        let tx = store.record(queued_transaction()).unwrap();

        store.claim_for_execution(&tx.transaction_id).unwrap();

        let found = store.find_by_request_hash("hash-abc").unwrap();
        assert!(
            found.is_none(),
            "Running transaction must not be returned (prevents duplicate execution)"
        );
    }

    #[test]
    fn find_by_request_hash_returns_queued_record_when_hash_shared_with_older_executed() {
        // If a preview was generated twice for the same action, the most recent
        // Queued record should be returned even if an older Succeeded record
        // exists for the same hash.
        let dir = tempdir().unwrap();
        let store = TransactionStore::open(dir.path().join("tx.db")).unwrap();

        // First round: record → execute → succeed.
        let first_tx = store.record(queued_transaction()).unwrap();
        store
            .update_status(&first_tx.transaction_id, JobState::Running)
            .unwrap();
        store
            .update_status(&first_tx.transaction_id, JobState::Succeeded)
            .unwrap();

        // Second round: new preview with same hash (still Queued).
        let second_tx = store.record(queued_transaction()).unwrap();

        let found = store.find_by_request_hash("hash-abc").unwrap();
        assert!(found.is_some(), "second Queued record should be found");
        assert_eq!(
            found.unwrap().transaction_id,
            second_tx.transaction_id,
            "should return the most-recent Queued record, not the older Succeeded one"
        );
    }

    // ── TTL expiry tests (issue #46) ────────────────────────────────────────

    #[test]
    fn fresh_queued_transaction_is_found_by_request_hash() {
        let dir = tempdir().unwrap();
        let store = TransactionStore::open(dir.path().join("tx.db")).unwrap();
        store.record(queued_transaction()).unwrap();

        let found = store.find_by_request_hash("hash-abc").unwrap();
        assert!(
            found.is_some(),
            "a freshly created Queued transaction must be found"
        );
    }

    #[test]
    fn stale_queued_transaction_is_not_found_by_request_hash() {
        let dir = tempdir().unwrap();
        let store = TransactionStore::open(dir.path().join("tx.db")).unwrap();
        let tx = store.record(queued_transaction()).unwrap();

        // Backdate created_at to 20 minutes ago so it exceeds the 15-minute TTL.
        let conn = store.connection().unwrap();
        conn.execute(
            "UPDATE transactions SET created_at = datetime('now', '-20 minutes') \
             WHERE transaction_id = ?1",
            params![tx.transaction_id],
        )
        .unwrap();

        let found = store.find_by_request_hash("hash-abc").unwrap();
        assert!(
            found.is_none(),
            "a Queued transaction older than 15 minutes must not be found"
        );
    }

    #[test]
    fn cleanup_stale_queued_cancels_old_records() {
        let dir = tempdir().unwrap();
        let store = TransactionStore::open(dir.path().join("tx.db")).unwrap();

        // Create two transactions: one fresh, one stale.
        let fresh = store.record(queued_transaction()).unwrap();
        let stale = store.record(queued_transaction()).unwrap();

        // Backdate the stale one.
        let conn = store.connection().unwrap();
        conn.execute(
            "UPDATE transactions SET created_at = datetime('now', '-20 minutes') \
             WHERE transaction_id = ?1",
            params![stale.transaction_id],
        )
        .unwrap();

        let canceled = store.cleanup_stale_queued().unwrap();
        assert_eq!(canceled, 1, "only the stale record should be canceled");

        // The stale record should now be Canceled.
        let stale_record = store.get(&stale.transaction_id).unwrap().unwrap();
        assert_eq!(stale_record.status, JobState::Canceled);

        // The fresh record should still be Queued.
        let fresh_record = store.get(&fresh.transaction_id).unwrap().unwrap();
        assert_eq!(fresh_record.status, JobState::Queued);
    }

    // ── State-machine validation tests (issue #56) ──────────────────────────

    #[test]
    fn update_status_rejects_queued_to_succeeded() {
        let dir = tempdir().unwrap();
        let store = TransactionStore::open(dir.path().join("tx.db")).unwrap();
        let tx = store.record(queued_transaction()).unwrap();

        let result = store.update_status(&tx.transaction_id, JobState::Succeeded);
        assert!(
            matches!(
                result,
                Err(TransactionStoreError::InvalidTransition {
                    from: JobState::Queued,
                    to: JobState::Succeeded,
                })
            ),
            "Queued -> Succeeded must be rejected (must go through Running first): {result:?}"
        );
    }

    #[test]
    fn update_status_rejects_succeeded_to_running() {
        let dir = tempdir().unwrap();
        let store = TransactionStore::open(dir.path().join("tx.db")).unwrap();
        let tx = store.record(queued_transaction()).unwrap();

        store
            .update_status(&tx.transaction_id, JobState::Running)
            .unwrap();
        store
            .update_status(&tx.transaction_id, JobState::Succeeded)
            .unwrap();

        let result = store.update_status(&tx.transaction_id, JobState::Running);
        assert!(
            matches!(
                result,
                Err(TransactionStoreError::InvalidTransition {
                    from: JobState::Succeeded,
                    to: JobState::Running,
                })
            ),
            "Succeeded -> Running must be rejected (terminal state): {result:?}"
        );
    }

    #[test]
    fn update_status_accepts_running_to_failed() {
        let dir = tempdir().unwrap();
        let store = TransactionStore::open(dir.path().join("tx.db")).unwrap();
        let tx = store.record(queued_transaction()).unwrap();

        store
            .update_status(&tx.transaction_id, JobState::Running)
            .unwrap();
        store
            .update_status(&tx.transaction_id, JobState::Failed)
            .unwrap();

        let updated = store.get(&tx.transaction_id).unwrap().unwrap();
        assert_eq!(updated.status, JobState::Failed);
    }

    #[test]
    fn update_status_accepts_running_to_rolled_back() {
        let dir = tempdir().unwrap();
        let store = TransactionStore::open(dir.path().join("tx.db")).unwrap();
        let tx = store.record(queued_transaction()).unwrap();

        store
            .update_status(&tx.transaction_id, JobState::Running)
            .unwrap();
        store
            .update_status(&tx.transaction_id, JobState::RolledBack)
            .unwrap();

        let updated = store.get(&tx.transaction_id).unwrap().unwrap();
        assert_eq!(updated.status, JobState::RolledBack);
    }

    // ── list_transactions tests ───────────────────────────────────────────

    #[test]
    fn list_transactions_returns_empty_for_fresh_store() {
        let dir = tempdir().unwrap();
        let store = TransactionStore::open(dir.path().join("tx.db")).unwrap();
        let results = store.list_transactions(10, None, None, None).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn list_transactions_returns_all_records_ordered_by_newest_first() {
        let dir = tempdir().unwrap();
        let store = TransactionStore::open(dir.path().join("tx.db")).unwrap();
        store.record(queued_transaction()).unwrap();

        let mut second = queued_transaction();
        second.action_name = "GetDiskUsage".to_string();
        second.risk_level = RiskLevel::Low;
        store.record(second).unwrap();

        let results = store.list_transactions(10, None, None, None).unwrap();
        assert_eq!(results.len(), 2);
        // Most recent first (GetDiskUsage was recorded second).
        assert_eq!(results[0].action_name, "GetDiskUsage");
        assert_eq!(results[1].action_name, "UpdateSystem");
    }

    #[test]
    fn list_transactions_respects_limit() {
        let dir = tempdir().unwrap();
        let store = TransactionStore::open(dir.path().join("tx.db")).unwrap();
        for _ in 0..5 {
            store.record(queued_transaction()).unwrap();
        }
        let results = store.list_transactions(3, None, None, None).unwrap();
        assert_eq!(results.len(), 3);
    }

    #[test]
    fn list_transactions_filters_by_status() {
        let dir = tempdir().unwrap();
        let store = TransactionStore::open(dir.path().join("tx.db")).unwrap();
        let tx = store.record(queued_transaction()).unwrap();
        store
            .update_status(&tx.transaction_id, JobState::Running)
            .unwrap();
        store
            .update_status(&tx.transaction_id, JobState::Succeeded)
            .unwrap();

        // Add another that stays Queued.
        store.record(queued_transaction()).unwrap();

        let succeeded = store
            .list_transactions(10, Some("succeeded"), None, None)
            .unwrap();
        assert_eq!(succeeded.len(), 1);
        assert_eq!(succeeded[0].status, JobState::Succeeded);

        let queued = store
            .list_transactions(10, Some("queued"), None, None)
            .unwrap();
        assert_eq!(queued.len(), 1);
        assert_eq!(queued[0].status, JobState::Queued);
    }

    #[test]
    fn list_transactions_filters_by_action_name() {
        let dir = tempdir().unwrap();
        let store = TransactionStore::open(dir.path().join("tx.db")).unwrap();
        store.record(queued_transaction()).unwrap(); // UpdateSystem

        let mut disk = queued_transaction();
        disk.action_name = "GetDiskUsage".to_string();
        store.record(disk).unwrap();

        let results = store
            .list_transactions(10, None, Some("GetDiskUsage"), None)
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].action_name, "GetDiskUsage");
    }

    #[test]
    fn list_transactions_filters_by_since_hours() {
        let dir = tempdir().unwrap();
        let store = TransactionStore::open(dir.path().join("tx.db")).unwrap();

        // Record a transaction and backdate it to 48 hours ago.
        let old = store.record(queued_transaction()).unwrap();
        let conn = store.connection().unwrap();
        conn.execute(
            "UPDATE transactions SET created_at = datetime('now', '-48 hours') \
             WHERE transaction_id = ?1",
            params![old.transaction_id],
        )
        .unwrap();

        // Record a fresh transaction.
        store.record(queued_transaction()).unwrap();

        // since_hours=24 should only return the fresh one.
        let results = store.list_transactions(10, None, None, Some(24)).unwrap();
        assert_eq!(results.len(), 1);

        // since_hours=72 should return both.
        let results = store.list_transactions(10, None, None, Some(72)).unwrap();
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn list_transactions_rejects_invalid_status_filter() {
        let dir = tempdir().unwrap();
        let store = TransactionStore::open(dir.path().join("tx.db")).unwrap();
        store.record(queued_transaction()).unwrap();
        let result = store.list_transactions(10, Some("bogus"), None, None);
        assert!(result.is_err(), "invalid status filter should return error");
    }
}
