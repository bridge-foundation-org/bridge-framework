//! Schema Introspection & Migration Validation
//!
//! Inspect schemas and validate migrations for compatibility

// Parts of this module are forward-scaffolding: their public API is
// intentionally ahead of its call sites. Trim this allow item-by-item as the
// dead surface shrinks.
#![allow(dead_code)]

use std::collections::HashMap;

/// Database column
#[derive(Clone, Debug)]
pub struct Column {
    pub name: String,
    pub data_type: String,
    pub nullable: bool,
    pub default: Option<String>,
    pub primary_key: bool,
}

impl Column {
    pub fn new(name: impl Into<String>, data_type: impl Into<String>) -> Self {
        Column {
            name: name.into(),
            data_type: data_type.into(),
            nullable: true,
            default: None,
            primary_key: false,
        }
    }

    pub fn not_null(mut self) -> Self {
        self.nullable = false;
        self
    }

    pub fn with_default(mut self, default: impl Into<String>) -> Self {
        self.default = Some(default.into());
        self
    }

    pub fn primary(mut self) -> Self {
        self.primary_key = true;
        self.nullable = false;
        self
    }
}

/// Database table schema
#[derive(Clone, Debug)]
pub struct TableSchema {
    pub name: String,
    pub columns: Vec<Column>,
    pub indexes: Vec<String>,
}

impl TableSchema {
    pub fn new(name: impl Into<String>) -> Self {
        TableSchema {
            name: name.into(),
            columns: Vec::new(),
            indexes: Vec::new(),
        }
    }

    pub fn add_column(mut self, col: Column) -> Self {
        self.columns.push(col);
        self
    }

    pub fn add_index(mut self, index_name: impl Into<String>) -> Self {
        self.indexes.push(index_name.into());
        self
    }

    pub fn get_column(&self, name: &str) -> Option<&Column> {
        self.columns.iter().find(|c| c.name == name)
    }

    pub fn has_column(&self, name: &str) -> bool {
        self.columns.iter().any(|c| c.name == name)
    }
}

/// Migration operation
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MigrationOp {
    CreateTable,
    DropTable,
    AddColumn,
    DropColumn,
    ModifyColumn,
    AddIndex,
    DropIndex,
}

impl MigrationOp {
    pub fn as_str(&self) -> &'static str {
        match self {
            MigrationOp::CreateTable => "create_table",
            MigrationOp::DropTable => "drop_table",
            MigrationOp::AddColumn => "add_column",
            MigrationOp::DropColumn => "drop_column",
            MigrationOp::ModifyColumn => "modify_column",
            MigrationOp::AddIndex => "add_index",
            MigrationOp::DropIndex => "drop_index",
        }
    }
}

/// Migration step
#[derive(Clone, Debug)]
pub struct MigrationStep {
    pub operation: MigrationOp,
    pub table: String,
    pub details: HashMap<String, String>,
}

impl MigrationStep {
    pub fn new(op: MigrationOp, table: impl Into<String>) -> Self {
        MigrationStep {
            operation: op,
            table: table.into(),
            details: HashMap::new(),
        }
    }

    pub fn with_detail(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.details.insert(key.into(), value.into());
        self
    }
}

/// Migration validation result
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ValidationResult {
    Valid,
    Warning,
    Error,
}

impl ValidationResult {
    pub fn as_str(&self) -> &'static str {
        match self {
            ValidationResult::Valid => "valid",
            ValidationResult::Warning => "warning",
            ValidationResult::Error => "error",
        }
    }
}

/// Schema validator
pub struct SchemaValidator {
    current_schema: HashMap<String, TableSchema>,
}

impl SchemaValidator {
    pub fn new() -> Self {
        SchemaValidator {
            current_schema: HashMap::new(),
        }
    }

    /// Register table schema
    pub fn register_table(&mut self, schema: TableSchema) {
        self.current_schema.insert(schema.name.clone(), schema);
    }

    /// Validate migration
    pub fn validate_migration(&self, migration: &MigrationStep) -> ValidationResult {
        match migration.operation {
            MigrationOp::CreateTable => {
                if self.current_schema.contains_key(&migration.table) {
                    ValidationResult::Error
                } else {
                    ValidationResult::Valid
                }
            }
            MigrationOp::DropTable => {
                if self.current_schema.contains_key(&migration.table) {
                    ValidationResult::Valid
                } else {
                    ValidationResult::Error
                }
            }
            MigrationOp::AddColumn | MigrationOp::DropColumn | MigrationOp::ModifyColumn => {
                if let Some(table) = self.current_schema.get(&migration.table) {
                    if let Some(column_name) = migration.details.get("column") {
                        match migration.operation {
                            MigrationOp::AddColumn => {
                                if table.has_column(column_name) {
                                    ValidationResult::Error
                                } else {
                                    ValidationResult::Valid
                                }
                            }
                            MigrationOp::DropColumn | MigrationOp::ModifyColumn => {
                                if table.has_column(column_name) {
                                    ValidationResult::Valid
                                } else {
                                    ValidationResult::Error
                                }
                            }
                            _ => ValidationResult::Valid,
                        }
                    } else {
                        ValidationResult::Error
                    }
                } else {
                    ValidationResult::Error
                }
            }
            _ => ValidationResult::Valid,
        }
    }

    /// Validate multiple migrations
    pub fn validate_all(&self, migrations: &[MigrationStep]) -> Vec<ValidationResult> {
        migrations
            .iter()
            .map(|m| self.validate_migration(m))
            .collect()
    }

    /// Get table schema
    pub fn get_table(&self, name: &str) -> Option<&TableSchema> {
        self.current_schema.get(name)
    }

    /// List all tables
    pub fn list_tables(&self) -> Vec<&TableSchema> {
        self.current_schema.values().collect()
    }

    /// Check if table exists
    pub fn table_exists(&self, name: &str) -> bool {
        self.current_schema.contains_key(name)
    }
}

impl Default for SchemaValidator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_column_new() {
        let col = Column::new("id", "integer");
        assert_eq!(col.name, "id");
        assert_eq!(col.data_type, "integer");
        assert!(col.nullable);
    }

    #[test]
    fn test_column_not_null() {
        let col = Column::new("id", "integer").not_null();
        assert!(!col.nullable);
    }

    #[test]
    fn test_column_primary() {
        let col = Column::new("id", "integer").primary();
        assert!(col.primary_key);
        assert!(!col.nullable);
    }

    #[test]
    fn test_table_schema_new() {
        let schema = TableSchema::new("users");
        assert_eq!(schema.name, "users");
        assert_eq!(schema.columns.len(), 0);
    }

    #[test]
    fn test_table_schema_add_column() {
        let schema = TableSchema::new("users")
            .add_column(Column::new("id", "integer"))
            .add_column(Column::new("name", "varchar"));
        assert_eq!(schema.columns.len(), 2);
    }

    #[test]
    fn test_table_schema_has_column() {
        let schema = TableSchema::new("users").add_column(Column::new("id", "integer"));
        assert!(schema.has_column("id"));
        assert!(!schema.has_column("email"));
    }

    #[test]
    fn test_migration_op_as_str() {
        assert_eq!(MigrationOp::CreateTable.as_str(), "create_table");
        assert_eq!(MigrationOp::DropTable.as_str(), "drop_table");
        assert_eq!(MigrationOp::AddColumn.as_str(), "add_column");
    }

    #[test]
    fn test_migration_step_new() {
        let step = MigrationStep::new(MigrationOp::AddColumn, "users");
        assert_eq!(step.operation, MigrationOp::AddColumn);
        assert_eq!(step.table, "users");
    }

    #[test]
    fn test_migration_step_with_detail() {
        let step =
            MigrationStep::new(MigrationOp::AddColumn, "users").with_detail("column", "email");
        assert_eq!(step.details.get("column"), Some(&"email".to_string()));
    }

    #[test]
    fn test_validation_result_as_str() {
        assert_eq!(ValidationResult::Valid.as_str(), "valid");
        assert_eq!(ValidationResult::Error.as_str(), "error");
    }

    #[test]
    fn test_schema_validator_new() {
        let validator = SchemaValidator::new();
        assert_eq!(validator.list_tables().len(), 0);
    }

    #[test]
    fn test_schema_validator_register_table() {
        let mut validator = SchemaValidator::new();
        let schema = TableSchema::new("users");
        validator.register_table(schema);
        assert_eq!(validator.list_tables().len(), 1);
    }

    #[test]
    fn test_schema_validator_table_exists() {
        let mut validator = SchemaValidator::new();
        validator.register_table(TableSchema::new("users"));
        assert!(validator.table_exists("users"));
        assert!(!validator.table_exists("posts"));
    }

    #[test]
    fn test_schema_validator_validate_create_table() {
        let validator = SchemaValidator::new();
        let migration = MigrationStep::new(MigrationOp::CreateTable, "users");
        let result = validator.validate_migration(&migration);
        assert_eq!(result, ValidationResult::Valid);
    }

    #[test]
    fn test_schema_validator_validate_create_duplicate() {
        let mut validator = SchemaValidator::new();
        validator.register_table(TableSchema::new("users"));

        let migration = MigrationStep::new(MigrationOp::CreateTable, "users");
        let result = validator.validate_migration(&migration);
        assert_eq!(result, ValidationResult::Error);
    }

    #[test]
    fn test_schema_validator_validate_add_column() {
        let mut validator = SchemaValidator::new();
        let schema = TableSchema::new("users").add_column(Column::new("id", "integer"));
        validator.register_table(schema);

        let migration =
            MigrationStep::new(MigrationOp::AddColumn, "users").with_detail("column", "email");
        let result = validator.validate_migration(&migration);
        assert_eq!(result, ValidationResult::Valid);
    }

    #[test]
    fn test_schema_validator_validate_add_duplicate_column() {
        let mut validator = SchemaValidator::new();
        let schema = TableSchema::new("users").add_column(Column::new("email", "varchar"));
        validator.register_table(schema);

        let migration =
            MigrationStep::new(MigrationOp::AddColumn, "users").with_detail("column", "email");
        let result = validator.validate_migration(&migration);
        assert_eq!(result, ValidationResult::Error);
    }

    #[test]
    fn test_schema_validator_validate_drop_column() {
        let mut validator = SchemaValidator::new();
        let schema = TableSchema::new("users").add_column(Column::new("email", "varchar"));
        validator.register_table(schema);

        let migration =
            MigrationStep::new(MigrationOp::DropColumn, "users").with_detail("column", "email");
        let result = validator.validate_migration(&migration);
        assert_eq!(result, ValidationResult::Valid);
    }

    #[test]
    fn test_schema_validator_validate_drop_nonexistent_column() {
        let mut validator = SchemaValidator::new();
        validator.register_table(TableSchema::new("users"));

        let migration =
            MigrationStep::new(MigrationOp::DropColumn, "users").with_detail("column", "email");
        let result = validator.validate_migration(&migration);
        assert_eq!(result, ValidationResult::Error);
    }

    #[test]
    fn test_schema_validator_validate_all() {
        let mut validator = SchemaValidator::new();
        validator.register_table(TableSchema::new("users"));

        let migrations = vec![
            MigrationStep::new(MigrationOp::AddColumn, "users").with_detail("column", "email"),
            MigrationStep::new(MigrationOp::DropTable, "posts"),
        ];

        let results = validator.validate_all(&migrations);
        assert_eq!(results.len(), 2);
        assert_eq!(results[0], ValidationResult::Valid);
        assert_eq!(results[1], ValidationResult::Error);
    }
}
