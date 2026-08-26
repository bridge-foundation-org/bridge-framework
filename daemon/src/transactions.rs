//! Transaction Management - ACID transaction support
//!
//! Multi-step transaction coordination with rollback

// Parts of this module are forward-scaffolding: their public API is
// intentionally ahead of its call sites. Trim this allow item-by-item as the
// dead surface shrinks.
#![allow(dead_code)]

use std::collections::HashMap;

/// Transaction state
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransactionState {
    Pending,
    Active,
    Committed,
    RolledBack,
    Failed,
}

impl TransactionState {
    pub fn as_str(&self) -> &'static str {
        match self {
            TransactionState::Pending => "pending",
            TransactionState::Active => "active",
            TransactionState::Committed => "committed",
            TransactionState::RolledBack => "rolled_back",
            TransactionState::Failed => "failed",
        }
    }
}

/// Transaction operation (legacy bookkeeping model)
#[allow(dead_code)]
#[derive(Clone, Debug)]
pub struct Operation {
    pub id: String,
    pub operation_type: String,
    pub data: HashMap<String, String>,
}

impl Operation {
    pub fn new(id: impl Into<String>, op_type: impl Into<String>) -> Self {
        Operation {
            id: id.into(),
            operation_type: op_type.into(),
            data: HashMap::new(),
        }
    }

    pub fn with_data(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.data.insert(key.into(), value.into());
        self
    }
}

/// Transaction
#[derive(Clone, Debug)]
pub struct Transaction {
    pub id: String,
    pub state: TransactionState,
    pub operations: Vec<Operation>,
}

impl Transaction {
    pub fn new(id: impl Into<String>) -> Self {
        Transaction {
            id: id.into(),
            state: TransactionState::Pending,
            operations: Vec::new(),
        }
    }

    pub fn add_operation(mut self, op: Operation) -> Self {
        self.operations.push(op);
        self
    }

    pub fn with_state(mut self, state: TransactionState) -> Self {
        self.state = state;
        self
    }
}

/// Transaction isolation level
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(dead_code)]
pub enum IsolationLevel {
    ReadUncommitted,
    ReadCommitted,
    RepeatableRead,
    Serializable,
}

impl IsolationLevel {
    pub fn as_str(&self) -> &'static str {
        match self {
            IsolationLevel::ReadUncommitted => "read_uncommitted",
            IsolationLevel::ReadCommitted => "read_committed",
            IsolationLevel::RepeatableRead => "repeatable_read",
            IsolationLevel::Serializable => "serializable",
        }
    }
}

/// Transaction manager
pub struct TransactionManager {
    transactions: HashMap<String, Transaction>,
    isolation_level: IsolationLevel,
    max_transactions: usize,
}

impl TransactionManager {
    pub fn new(isolation_level: IsolationLevel) -> Self {
        TransactionManager {
            transactions: HashMap::new(),
            isolation_level,
            max_transactions: 10000,
        }
    }

    /// Begin transaction
    pub fn begin(&mut self, tx_id: impl Into<String>) -> Result<String, String> {
        let id = tx_id.into();
        if self.transactions.contains_key(&id) {
            return Err(format!("Transaction {} already exists", id));
        }

        if self.transactions.len() >= self.max_transactions {
            return Err("Max transactions limit reached".to_string());
        }

        let tx = Transaction::new(&id).with_state(TransactionState::Active);
        self.transactions.insert(id.clone(), tx);
        Ok(id)
    }

    /// Add operation to transaction
    pub fn add_operation(&mut self, tx_id: &str, op: Operation) -> Result<(), String> {
        let tx = self
            .transactions
            .get_mut(tx_id)
            .ok_or_else(|| format!("Transaction {} not found", tx_id))?;

        if tx.state != TransactionState::Active {
            return Err(format!("Transaction not active: {:?}", tx.state));
        }

        tx.operations.push(op);
        Ok(())
    }

    /// Commit transaction
    pub fn commit(&mut self, tx_id: &str) -> Result<(), String> {
        let tx = self
            .transactions
            .get_mut(tx_id)
            .ok_or_else(|| format!("Transaction {} not found", tx_id))?;

        if tx.state != TransactionState::Active {
            return Err("Cannot commit non-active transaction".to_string());
        }

        tx.state = TransactionState::Committed;
        Ok(())
    }

    /// Rollback transaction
    pub fn rollback(&mut self, tx_id: &str) -> Result<(), String> {
        let tx = self
            .transactions
            .get_mut(tx_id)
            .ok_or_else(|| format!("Transaction {} not found", tx_id))?;

        tx.state = TransactionState::RolledBack;
        Ok(())
    }

    /// Get transaction
    pub fn get_transaction(&self, tx_id: &str) -> Option<&Transaction> {
        self.transactions.get(tx_id)
    }

    /// List active transactions
    pub fn list_active(&self) -> Vec<&Transaction> {
        self.transactions
            .values()
            .filter(|tx| tx.state == TransactionState::Active)
            .collect()
    }

    /// Get isolation level
    pub fn isolation_level(&self) -> IsolationLevel {
        self.isolation_level
    }
}

impl Default for TransactionManager {
    fn default() -> Self {
        Self::new(IsolationLevel::ReadCommitted)
    }
}

// ── Store-backed transactions ─────────────────────────────────────────────────
//
// The bookkeeping API above tracks transaction *state*; this layer makes a
// transaction actually *do* something: queued operations are applied to the
// daemon's KV store (`db` crate) on commit and discarded on rollback.

/// A queued mutation against the KV store.
#[derive(Debug, Clone, PartialEq)]
pub enum StoreOp {
    Put {
        ns: String,
        key: String,
        value: String,
    },
    Del {
        ns: String,
        key: String,
    },
    /// Delete keys matching a glob (`*`, `?`) within one namespace.
    DelMatching {
        ns: String,
        pattern: String,
    },
}

impl StoreOp {
    pub fn kind(&self) -> &'static str {
        match self {
            StoreOp::Put { .. } => "put",
            StoreOp::Del { .. } => "del",
            StoreOp::DelMatching { .. } => "del_matching",
        }
    }

    pub fn to_json(&self) -> String {
        match self {
            StoreOp::Put { ns, key, value } => format!(
                r#"{{"op":"put","ns":"{ns}","key":"{key}","value":{v}}}"#,
                v = serde_json_value_string(value)
            ),
            StoreOp::Del { ns, key } => {
                format!(r#"{{"op":"del","ns":"{ns}","key":"{key}"}}"#)
            }
            StoreOp::DelMatching { ns, pattern } => {
                format!(r#"{{"op":"del_matching","ns":"{ns}","pattern":"{pattern}"}}"#)
            }
        }
    }
}

/// Render `value` as a JSON string literal (escaped, always quoted).
fn serde_json_value_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// A live store-backed transaction held in [`TxRegistry`].
#[derive(Debug, Clone)]
pub struct StoreTransaction {
    pub id: String,
    pub isolation: IsolationLevel,
    pub state: TransactionState,
    pub ops: Vec<StoreOp>,
    pub created_at_secs: u64,
}

/// Registry of store-backed transactions, keyed by id.
///
/// All methods take `&self`; interior locking makes it shareable through the
/// daemon `State` without extra synchronization at call sites.
#[derive(Default)]
pub struct TxRegistry {
    inner: std::sync::Mutex<HashMap<String, StoreTransaction>>,
}

impl TxRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Begin a new transaction. Fails if the id already exists or is active.
    pub fn begin(&self, id: impl Into<String>, isolation: IsolationLevel) -> Result<(), String> {
        let id = id.into();
        let mut g = self.inner.lock().unwrap();
        if g.contains_key(&id) {
            return Err(format!("transaction {id:?} already exists"));
        }
        g.insert(
            id.clone(),
            StoreTransaction {
                id,
                isolation,
                state: TransactionState::Active,
                ops: Vec::new(),
                created_at_secs: now_secs(),
            },
        );
        Ok(())
    }

    /// Queue an operation into an active transaction.
    pub fn enqueue(&self, tx_id: &str, op: StoreOp) -> Result<usize, String> {
        let mut g = self.inner.lock().unwrap();
        let tx = g
            .get_mut(tx_id)
            .ok_or_else(|| format!("transaction {tx_id:?} not found"))?;
        if tx.state != TransactionState::Active {
            return Err(format!(
                "transaction {tx_id:?} is {} — cannot enqueue",
                tx.state.as_str()
            ));
        }
        tx.ops.push(op);
        Ok(tx.ops.len())
    }

    /// Apply every queued operation to `store` atomically-in-order and mark
    /// committed. Returns the number of operations applied. On any hard error
    /// the transaction is marked Failed and nothing further is applied
    /// (operations already applied stay — matching read-committed semantics;
    /// callers may inspect state and retry).
    pub fn commit(&self, tx_id: &str, store: &db::Db) -> Result<usize, String> {
        let ops = {
            let mut g = self.inner.lock().unwrap();
            let tx = g
                .get_mut(tx_id)
                .ok_or_else(|| format!("transaction {tx_id:?} not found"))?;
            match tx.state {
                TransactionState::Active => {}
                other => {
                    return Err(format!(
                        "cannot commit transaction {tx_id:?} in state {other:?}"
                    ))
                }
            }
            tx.state = TransactionState::Committed;
            std::mem::take(&mut tx.ops)
        };
        for op in &ops {
            if let Err(e) = apply_op(store, op) {
                let mut g = self.inner.lock().unwrap();
                if let Some(tx) = g.get_mut(tx_id) {
                    tx.state = TransactionState::Failed;
                }
                return Err(format!("commit failed at {}: {e}", op.kind()));
            }
        }
        Ok(ops.len())
    }

    /// Discard all queued operations and mark rolled back.
    pub fn rollback(&self, tx_id: &str) -> Result<usize, String> {
        let mut g = self.inner.lock().unwrap();
        let tx = g
            .get_mut(tx_id)
            .ok_or_else(|| format!("transaction {tx_id:?} not found"))?;
        match tx.state {
            TransactionState::Active | TransactionState::Failed => {
                tx.state = TransactionState::RolledBack;
                Ok(std::mem::take(&mut tx.ops).len())
            }
            other => Err(format!(
                "cannot roll back transaction {tx_id:?} in state {other:?}"
            )),
        }
    }

    /// Look up a transaction snapshot.
    pub fn get(&self, tx_id: &str) -> Option<StoreTransaction> {
        self.inner.lock().unwrap().get(tx_id).cloned()
    }

    /// Ids of all transactions, sorted.
    pub fn ids(&self) -> Vec<String> {
        let g = self.inner.lock().unwrap();
        let mut ids: Vec<String> = g.keys().cloned().collect();
        ids.sort();
        ids
    }

    /// Drop terminal-state transactions (committed / rolled_back / failed).
    /// Returns how many were pruned.
    pub fn prune_finished(&self) -> usize {
        let mut g = self.inner.lock().unwrap();
        let before = g.len();
        g.retain(|_, tx| {
            tx.state == TransactionState::Active || tx.state == TransactionState::Pending
        });
        before - g.len()
    }

    /// JSON summary for `GET /api/v1/tx`.
    pub fn to_json(&self) -> String {
        let g = self.inner.lock().unwrap();
        let mut ids: Vec<&String> = g.keys().collect();
        ids.sort();
        let items: Vec<String> = ids
            .iter()
            .map(|id| {
                let tx = &g[*id];
                let ops: Vec<String> = tx.ops.iter().map(|o| o.to_json()).collect();
                format!(
                    r#"{{"id":"{id}","state":"{st}","isolation":"{iso}","ops":{n},"created_at":{ts},"queued":[{q}]}}"#,
                    st = tx.state.as_str(),
                    iso = tx.isolation.as_str(),
                    n = tx.ops.len(),
                    ts = tx.created_at_secs,
                    q = ops.join(","),
                )
            })
            .collect();
        let active = g
            .values()
            .filter(|t| t.state == TransactionState::Active)
            .count();
        format!(
            r#"{{"total":{},"active":{active},"items":[{}]}}"#,
            g.len(),
            items.join(",")
        )
    }
}

/// Apply one queued operation to the store.
fn apply_op(store: &db::Db, op: &StoreOp) -> Result<(), String> {
    match op {
        StoreOp::Put { ns, key, value } => {
            store.put(ns, key, value.clone());
            Ok(())
        }
        StoreOp::Del { ns, key } => {
            store.del(ns, key);
            Ok(())
        }
        StoreOp::DelMatching { ns, pattern } => {
            // Collect first, then delete — avoids mutating while iterating.
            let doomed = store.keys_matching(ns, pattern);
            for k in doomed {
                store.del(ns, &k);
            }
            Ok(())
        }
    }
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

// ── Store-transaction tests ───────────────────────────────────────────────────

#[cfg(test)]
mod store_tests {
    use super::*;

    #[test]
    fn begin_enqueue_commit_applies_puts_in_order() {
        let store = db::Db::new();
        let reg = TxRegistry::new();
        reg.begin("tx1", IsolationLevel::ReadCommitted).unwrap();
        assert_eq!(
            reg.enqueue(
                "tx1",
                StoreOp::Put {
                    ns: "app".into(),
                    key: "a".into(),
                    value: "1".into()
                }
            )
            .unwrap(),
            1
        );
        reg.enqueue(
            "tx1",
            StoreOp::Put {
                ns: "app".into(),
                key: "b".into(),
                value: "2".into(),
            },
        )
        .unwrap();

        let applied = reg.commit("tx1", &store).unwrap();
        assert_eq!(applied, 2);
        assert_eq!(store.get("app", "a").as_deref(), Some("1"));
        assert_eq!(store.get("app", "b").as_deref(), Some("2"));
        // Ops are drained after commit; state recorded.
        let tx = reg.get("tx1").unwrap();
        assert_eq!(tx.state, TransactionState::Committed);
        assert!(tx.ops.is_empty());
    }

    #[test]
    fn rollback_discards_queued_ops() {
        let store = db::Db::new();
        let reg = TxRegistry::new();
        reg.begin("tx2", IsolationLevel::Serializable).unwrap();
        reg.enqueue(
            "tx2",
            StoreOp::Put {
                ns: "n".into(),
                key: "k".into(),
                value: "v".into(),
            },
        )
        .unwrap();
        let dropped = reg.rollback("tx2").unwrap();
        assert_eq!(dropped, 1);
        assert_eq!(store.get("n", "k"), None);
    }

    #[test]
    fn del_and_del_matching_apply_on_commit() {
        let store = db::Db::new();
        store.put("n", "keep", "x");
        store.put("n", "tmp:1", "a");
        store.put("n", "tmp:2", "b");

        let reg = TxRegistry::new();
        reg.begin("tx3", IsolationLevel::ReadCommitted).unwrap();
        reg.enqueue(
            "tx3",
            StoreOp::DelMatching {
                ns: "n".into(),
                pattern: "tmp:*".into(),
            },
        )
        .unwrap();
        reg.enqueue(
            "tx3",
            StoreOp::Del {
                ns: "n".into(),
                key: "keep".into(),
            },
        )
        .unwrap();
        reg.commit("tx3", &store).unwrap();

        assert_eq!(store.get("n", "tmp:1"), None);
        assert_eq!(store.get("n", "tmp:2"), None);
        assert_eq!(store.get("n", "keep"), None);
    }

    #[test]
    fn duplicate_begin_rejected() {
        let reg = TxRegistry::new();
        reg.begin("dup", IsolationLevel::ReadCommitted).unwrap();
        assert!(reg.begin("dup", IsolationLevel::ReadCommitted).is_err());
    }

    #[test]
    fn enqueue_after_commit_rejected() {
        let store = db::Db::new();
        let reg = TxRegistry::new();
        reg.begin("late", IsolationLevel::ReadCommitted).unwrap();
        reg.commit("late", &store).unwrap();
        let err = reg
            .enqueue(
                "late",
                StoreOp::Put {
                    ns: "n".into(),
                    key: "k".into(),
                    value: "v".into(),
                },
            )
            .unwrap_err();
        assert!(err.contains("committed"));
    }

    #[test]
    fn commit_unknown_tx_errors() {
        let store = db::Db::new();
        let reg = TxRegistry::new();
        assert!(reg.commit("ghost", &store).is_err());
        assert!(reg.rollback("ghost").is_err());
    }

    #[test]
    fn double_commit_rejected() {
        let store = db::Db::new();
        let reg = TxRegistry::new();
        reg.begin("dc", IsolationLevel::ReadCommitted).unwrap();
        reg.commit("dc", &store).unwrap();
        assert!(reg.commit("dc", &store).is_err());
    }

    #[test]
    fn prune_removes_only_terminal_transactions() {
        let store = db::Db::new();
        let reg = TxRegistry::new();
        reg.begin("live", IsolationLevel::ReadCommitted).unwrap();
        reg.begin("done", IsolationLevel::ReadCommitted).unwrap();
        reg.commit("done", &store).unwrap();
        reg.begin("aborted", IsolationLevel::ReadCommitted).unwrap();
        reg.rollback("aborted").unwrap();

        assert_eq!(reg.prune_finished(), 2);
        assert_eq!(reg.ids(), vec!["live".to_string()]);
    }

    #[test]
    fn json_summary_lists_sorted_with_ops() {
        let reg = TxRegistry::new();
        reg.begin("b-second", IsolationLevel::Serializable).unwrap();
        reg.begin("a-first", IsolationLevel::ReadCommitted).unwrap();
        reg.enqueue(
            "a-first",
            StoreOp::Put {
                ns: "n".into(),
                key: "k".into(),
                value: "has \"quote\"".into(),
            },
        )
        .unwrap();
        let json = reg.to_json();
        assert!(json.contains(r#""total":2"#));
        assert!(json.contains(r#""active":2"#));
        let a = json.find("a-first").unwrap();
        let b = json.find("b-second").unwrap();
        assert!(a < b, "ids sorted");
        assert!(json.contains(r#""value":"has \"quote\"""#));
    }

    #[test]
    fn store_op_json_shapes() {
        assert_eq!(
            StoreOp::Del {
                ns: "n".into(),
                key: "k".into()
            }
            .to_json(),
            r#"{"op":"del","ns":"n","key":"k"}"#
        );
        assert_eq!(
            StoreOp::DelMatching {
                ns: "n".into(),
                pattern: "t*".into()
            }
            .to_json(),
            r#"{"op":"del_matching","ns":"n","pattern":"t*"}"#
        );
        assert_eq!(
            StoreOp::Put {
                ns: "n".into(),
                key: "k".into(),
                value: "v".into()
            }
            .kind(),
            "put"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_transaction_state_as_str() {
        assert_eq!(TransactionState::Active.as_str(), "active");
        assert_eq!(TransactionState::Committed.as_str(), "committed");
    }

    #[test]
    fn test_operation_new() {
        let op = Operation::new("op1", "insert");
        assert_eq!(op.id, "op1");
        assert_eq!(op.operation_type, "insert");
    }

    #[test]
    fn test_operation_with_data() {
        let op = Operation::new("op1", "insert").with_data("key", "value");
        assert_eq!(op.data.get("key"), Some(&"value".to_string()));
    }

    #[test]
    fn test_transaction_new() {
        let tx = Transaction::new("tx1");
        assert_eq!(tx.id, "tx1");
        assert_eq!(tx.state, TransactionState::Pending);
    }

    #[test]
    fn test_transaction_add_operation() {
        let op = Operation::new("op1", "insert");
        let tx = Transaction::new("tx1").add_operation(op);
        assert_eq!(tx.operations.len(), 1);
    }

    #[test]
    fn test_isolation_level_as_str() {
        assert_eq!(IsolationLevel::Serializable.as_str(), "serializable");
        assert_eq!(IsolationLevel::ReadCommitted.as_str(), "read_committed");
    }

    #[test]
    fn test_transaction_manager_new() {
        let tm = TransactionManager::new(IsolationLevel::Serializable);
        assert_eq!(tm.isolation_level(), IsolationLevel::Serializable);
    }

    #[test]
    fn test_transaction_manager_begin() {
        let mut tm = TransactionManager::new(IsolationLevel::ReadCommitted);
        let result = tm.begin("tx1");
        assert!(result.is_ok());
    }

    #[test]
    fn test_transaction_manager_begin_duplicate() {
        let mut tm = TransactionManager::new(IsolationLevel::ReadCommitted);
        tm.begin("tx1").unwrap();
        let result = tm.begin("tx1");
        assert!(result.is_err());
    }

    #[test]
    fn test_transaction_manager_add_operation() {
        let mut tm = TransactionManager::new(IsolationLevel::ReadCommitted);
        tm.begin("tx1").unwrap();

        let op = Operation::new("op1", "insert");
        let result = tm.add_operation("tx1", op);
        assert!(result.is_ok());
    }

    #[test]
    fn test_transaction_manager_add_operation_not_found() {
        let mut tm = TransactionManager::new(IsolationLevel::ReadCommitted);
        let op = Operation::new("op1", "insert");
        let result = tm.add_operation("nonexistent", op);
        assert!(result.is_err());
    }

    #[test]
    fn test_transaction_manager_commit() {
        let mut tm = TransactionManager::new(IsolationLevel::ReadCommitted);
        tm.begin("tx1").unwrap();
        let result = tm.commit("tx1");
        assert!(result.is_ok());

        let tx = tm.get_transaction("tx1").unwrap();
        assert_eq!(tx.state, TransactionState::Committed);
    }

    #[test]
    fn test_transaction_manager_rollback() {
        let mut tm = TransactionManager::new(IsolationLevel::ReadCommitted);
        tm.begin("tx1").unwrap();
        let result = tm.rollback("tx1");
        assert!(result.is_ok());

        let tx = tm.get_transaction("tx1").unwrap();
        assert_eq!(tx.state, TransactionState::RolledBack);
    }

    #[test]
    fn test_transaction_manager_get_transaction() {
        let mut tm = TransactionManager::new(IsolationLevel::ReadCommitted);
        tm.begin("tx1").unwrap();
        let tx = tm.get_transaction("tx1");
        assert!(tx.is_some());
    }

    #[test]
    fn test_transaction_manager_list_active() {
        let mut tm = TransactionManager::new(IsolationLevel::ReadCommitted);
        tm.begin("tx1").unwrap();
        tm.begin("tx2").unwrap();
        tm.begin("tx3").unwrap();
        tm.commit("tx2").unwrap();

        let active = tm.list_active();
        assert_eq!(active.len(), 2);
    }

    #[test]
    fn test_transaction_manager_full_workflow() {
        let mut tm = TransactionManager::new(IsolationLevel::Serializable);

        tm.begin("tx1").unwrap();

        let op1 = Operation::new("op1", "insert").with_data("id", "1");
        let op2 = Operation::new("op2", "update").with_data("id", "2");

        tm.add_operation("tx1", op1).unwrap();
        tm.add_operation("tx1", op2).unwrap();

        let tx = tm.get_transaction("tx1").unwrap();
        assert_eq!(tx.operations.len(), 2);

        tm.commit("tx1").unwrap();

        let tx = tm.get_transaction("tx1").unwrap();
        assert_eq!(tx.state, TransactionState::Committed);
    }
}
