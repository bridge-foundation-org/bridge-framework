//! Transaction Management - ACID transaction support
//!
//! Multi-step transaction coordination with rollback

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

/// Transaction operation
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
        let tx = self.transactions.get_mut(tx_id)
            .ok_or_else(|| format!("Transaction {} not found", tx_id))?;

        if tx.state != TransactionState::Active {
            return Err(format!("Transaction not active: {:?}", tx.state));
        }

        tx.operations.push(op);
        Ok(())
    }

    /// Commit transaction
    pub fn commit(&mut self, tx_id: &str) -> Result<(), String> {
        let tx = self.transactions.get_mut(tx_id)
            .ok_or_else(|| format!("Transaction {} not found", tx_id))?;

        if tx.state != TransactionState::Active {
            return Err(format!("Cannot commit non-active transaction"));
        }

        tx.state = TransactionState::Committed;
        Ok(())
    }

    /// Rollback transaction
    pub fn rollback(&mut self, tx_id: &str) -> Result<(), String> {
        let tx = self.transactions.get_mut(tx_id)
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
        let op = Operation::new("op1", "insert")
            .with_data("key", "value");
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
