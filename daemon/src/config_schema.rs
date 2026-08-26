//! Config Schema Generation and Validation
//!
//! Generates validation schemas for configuration, enabling type-safe config loading
//! and runtime validation with helpful error messages.

// Parts of this module are forward-scaffolding: their public API is
// intentionally ahead of its call sites. Trim this allow item-by-item as the
// dead surface shrinks.
#![allow(dead_code)]

use std::collections::HashMap;

/// Represents a configuration value type
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ConfigType {
    /// String value
    String,
    /// Integer value
    Integer,
    /// Floating point value
    Float,
    /// Boolean value
    Boolean,
    /// Array of values
    Array(Box<ConfigType>),
    /// Object with named fields
    Object(HashMap<String, ConfigType>),
    /// Optional value (nullable)
    Optional(Box<ConfigType>),
}

impl ConfigType {
    /// Get the type name as a string
    pub fn type_name(&self) -> &'static str {
        match self {
            ConfigType::String => "string",
            ConfigType::Integer => "integer",
            ConfigType::Float => "float",
            ConfigType::Boolean => "boolean",
            ConfigType::Array(_) => "array",
            ConfigType::Object(_) => "object",
            ConfigType::Optional(_) => "optional",
        }
    }

    /// Check if this type is optional
    pub fn is_optional(&self) -> bool {
        matches!(self, ConfigType::Optional(_))
    }

    /// Unwrap optional type
    pub fn unwrap_optional(&self) -> Option<&ConfigType> {
        match self {
            ConfigType::Optional(inner) => Some(inner),
            _ => None,
        }
    }
}

/// Configuration field validation constraints
#[derive(Clone, Debug)]
pub struct ConfigConstraint {
    /// Minimum value (for numbers)
    pub min: Option<i64>,
    /// Maximum value (for numbers)
    pub max: Option<i64>,
    /// Minimum length (for strings/arrays)
    pub min_length: Option<usize>,
    /// Maximum length (for strings/arrays)
    pub max_length: Option<usize>,
    /// Regular expression pattern (for strings)
    pub pattern: Option<String>,
    /// Allowed enum values
    pub enum_values: Option<Vec<String>>,
    /// Custom validation error message
    pub error_message: Option<String>,
}

impl ConfigConstraint {
    /// Create a new empty constraint
    pub fn new() -> Self {
        ConfigConstraint {
            min: None,
            max: None,
            min_length: None,
            max_length: None,
            pattern: None,
            enum_values: None,
            error_message: None,
        }
    }

    /// Add minimum value constraint
    pub fn with_min(mut self, min: i64) -> Self {
        self.min = Some(min);
        self
    }

    /// Add maximum value constraint
    pub fn with_max(mut self, max: i64) -> Self {
        self.max = Some(max);
        self
    }

    /// Add minimum length constraint
    pub fn with_min_length(mut self, length: usize) -> Self {
        self.min_length = Some(length);
        self
    }

    /// Add maximum length constraint
    pub fn with_max_length(mut self, length: usize) -> Self {
        self.max_length = Some(length);
        self
    }

    /// Add pattern constraint
    pub fn with_pattern(mut self, pattern: impl Into<String>) -> Self {
        self.pattern = Some(pattern.into());
        self
    }

    /// Add enum values constraint
    pub fn with_enum(mut self, values: Vec<String>) -> Self {
        self.enum_values = Some(values);
        self
    }

    /// Add custom error message
    pub fn with_error_message(mut self, message: impl Into<String>) -> Self {
        self.error_message = Some(message.into());
        self
    }
}

impl Default for ConfigConstraint {
    fn default() -> Self {
        Self::new()
    }
}

/// Configuration field schema definition
#[derive(Clone, Debug)]
pub struct ConfigField {
    /// Field name
    pub name: String,
    /// Field type
    pub field_type: ConfigType,
    /// Field description
    pub description: Option<String>,
    /// Default value (as string)
    pub default: Option<String>,
    /// Whether field is required
    pub required: bool,
    /// Validation constraints
    pub constraints: ConfigConstraint,
}

impl ConfigField {
    /// Create a new config field
    pub fn new(name: impl Into<String>, field_type: ConfigType) -> Self {
        ConfigField {
            name: name.into(),
            field_type,
            description: None,
            default: None,
            required: true,
            constraints: ConfigConstraint::new(),
        }
    }

    /// Make field optional
    pub fn optional(mut self) -> Self {
        self.required = false;
        self
    }

    /// Set description
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Set default value
    pub fn with_default(mut self, default: impl Into<String>) -> Self {
        self.default = Some(default.into());
        self
    }

    /// Set constraints
    pub fn with_constraints(mut self, constraints: ConfigConstraint) -> Self {
        self.constraints = constraints;
        self
    }
}

/// Configuration schema definition
#[derive(Clone, Debug)]
pub struct ConfigSchema {
    /// Schema name
    pub name: String,
    /// Schema version
    pub version: String,
    /// Schema description
    pub description: Option<String>,
    /// Fields in the schema
    pub fields: Vec<ConfigField>,
}

impl ConfigSchema {
    /// Create a new config schema
    pub fn new(name: impl Into<String>, version: impl Into<String>) -> Self {
        ConfigSchema {
            name: name.into(),
            version: version.into(),
            description: None,
            fields: Vec::new(),
        }
    }

    /// Add a field to the schema
    pub fn field(mut self, field: ConfigField) -> Self {
        self.fields.push(field);
        self
    }

    /// Set description
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Get a field by name
    pub fn get_field(&self, name: &str) -> Option<&ConfigField> {
        self.fields.iter().find(|f| f.name == name)
    }

    /// Validate a configuration value
    pub fn validate(&self, values: &HashMap<String, String>) -> Result<(), Vec<String>> {
        let mut errors = Vec::new();

        // Check for required fields
        for field in &self.fields {
            if field.required && !values.contains_key(&field.name) && field.default.is_none() {
                errors.push(format!("Required field '{}' is missing", field.name));
            }
        }

        // Validate provided values
        for (key, value) in values {
            if let Some(field) = self.get_field(key) {
                if let Err(err) = self.validate_field(field, value) {
                    errors.push(err);
                }
            } else {
                errors.push(format!("Unknown configuration field '{}'", key));
            }
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }

    /// Validate a single field value
    fn validate_field(&self, field: &ConfigField, value: &str) -> Result<(), String> {
        // Type validation
        match &field.field_type {
            ConfigType::String => {
                // Validate string constraints
                if let Some(min_len) = field.constraints.min_length {
                    if value.len() < min_len {
                        return Err(format!(
                            "Field '{}' minimum length is {}",
                            field.name, min_len
                        ));
                    }
                }
                if let Some(max_len) = field.constraints.max_length {
                    if value.len() > max_len {
                        return Err(format!(
                            "Field '{}' maximum length is {}",
                            field.name, max_len
                        ));
                    }
                }
                if let Some(enum_vals) = &field.constraints.enum_values {
                    if !enum_vals.contains(&value.to_string()) {
                        return Err(format!(
                            "Field '{}' must be one of: {}",
                            field.name,
                            enum_vals.join(", ")
                        ));
                    }
                }
            }
            ConfigType::Integer => {
                if let Ok(num) = value.parse::<i64>() {
                    if let Some(min) = field.constraints.min {
                        if num < min {
                            return Err(format!("Field '{}' minimum value is {}", field.name, min));
                        }
                    }
                    if let Some(max) = field.constraints.max {
                        if num > max {
                            return Err(format!("Field '{}' maximum value is {}", field.name, max));
                        }
                    }
                } else {
                    return Err(format!("Field '{}' must be an integer", field.name));
                }
            }
            ConfigType::Float => {
                if value.parse::<f64>().is_err() {
                    return Err(format!("Field '{}' must be a float", field.name));
                }
            }
            ConfigType::Boolean if !matches!(value.to_lowercase().as_str(), "true" | "false") => {
                return Err(format!("Field '{}' must be true or false", field.name));
            }
            _ => {
                // Complex types not validated here
            }
        }

        // Custom error message
        if let Some(error_msg) = &field.constraints.error_message {
            return Err(error_msg.clone());
        }

        Ok(())
    }

    /// Generate JSON Schema representation
    pub fn to_json_schema(&self) -> String {
        let mut schema = String::from("{\n");
        schema.push_str(&format!("  \"name\": \"{}\",\n", self.name));
        schema.push_str(&format!("  \"version\": \"{}\",\n", self.version));

        if let Some(desc) = &self.description {
            schema.push_str(&format!("  \"description\": \"{}\",\n", desc));
        }

        schema.push_str("  \"properties\": {\n");

        for (i, field) in self.fields.iter().enumerate() {
            schema.push_str(&format!("    \"{}\": {{\n", field.name));
            schema.push_str(&format!(
                "      \"type\": \"{}\",\n",
                field.field_type.type_name()
            ));

            if let Some(desc) = &field.description {
                schema.push_str(&format!("      \"description\": \"{}\",\n", desc));
            }

            if let Some(default) = &field.default {
                schema.push_str(&format!("      \"default\": \"{}\",\n", default));
            }

            schema.push_str(&format!("      \"required\": {}\n", field.required));
            schema.push_str("    }");

            if i < self.fields.len() - 1 {
                schema.push(',');
            }
            schema.push('\n');
        }

        schema.push_str("  }\n");
        schema.push('}');

        schema
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_type_string() {
        let t = ConfigType::String;
        assert_eq!(t.type_name(), "string");
        assert!(!t.is_optional());
    }

    #[test]
    fn test_config_type_integer() {
        let t = ConfigType::Integer;
        assert_eq!(t.type_name(), "integer");
    }

    #[test]
    fn test_config_type_optional() {
        let t = ConfigType::Optional(Box::new(ConfigType::String));
        assert!(t.is_optional());
        assert_eq!(t.unwrap_optional().unwrap().type_name(), "string");
    }

    #[test]
    fn test_config_constraint_new() {
        let c = ConfigConstraint::new();
        assert!(c.min.is_none());
        assert!(c.max.is_none());
    }

    #[test]
    fn test_config_constraint_min_max() {
        let c = ConfigConstraint::new().with_min(0).with_max(100);
        assert_eq!(c.min, Some(0));
        assert_eq!(c.max, Some(100));
    }

    #[test]
    fn test_config_constraint_length() {
        let c = ConfigConstraint::new()
            .with_min_length(5)
            .with_max_length(50);
        assert_eq!(c.min_length, Some(5));
        assert_eq!(c.max_length, Some(50));
    }

    #[test]
    fn test_config_constraint_enum() {
        let c = ConfigConstraint::new().with_enum(vec!["dev".into(), "prod".into(), "test".into()]);
        assert_eq!(c.enum_values.as_ref().unwrap().len(), 3);
    }

    #[test]
    fn test_config_field_new() {
        let field = ConfigField::new("port", ConfigType::Integer);
        assert_eq!(field.name, "port");
        assert!(field.required);
    }

    #[test]
    fn test_config_field_optional() {
        let field = ConfigField::new("debug", ConfigType::Boolean).optional();
        assert!(!field.required);
    }

    #[test]
    fn test_config_field_with_default() {
        let field = ConfigField::new("port", ConfigType::Integer).with_default("8080");
        assert_eq!(field.default, Some("8080".to_string()));
    }

    #[test]
    fn test_config_field_with_description() {
        let field =
            ConfigField::new("port", ConfigType::Integer).with_description("Server port number");
        assert_eq!(field.description, Some("Server port number".to_string()));
    }

    #[test]
    fn test_config_schema_new() {
        let schema = ConfigSchema::new("app", "1.0.0");
        assert_eq!(schema.name, "app");
        assert_eq!(schema.version, "1.0.0");
        assert!(schema.fields.is_empty());
    }

    #[test]
    fn test_config_schema_add_field() {
        let schema = ConfigSchema::new("app", "1.0.0")
            .field(ConfigField::new("port", ConfigType::Integer))
            .field(ConfigField::new("debug", ConfigType::Boolean));

        assert_eq!(schema.fields.len(), 2);
    }

    #[test]
    fn test_config_schema_get_field() {
        let schema =
            ConfigSchema::new("app", "1.0.0").field(ConfigField::new("port", ConfigType::Integer));

        let field = schema.get_field("port");
        assert!(field.is_some());
        assert_eq!(field.unwrap().name, "port");
    }

    #[test]
    fn test_config_schema_get_field_not_found() {
        let schema = ConfigSchema::new("app", "1.0.0");
        assert!(schema.get_field("port").is_none());
    }

    #[test]
    fn test_config_schema_validate_required_field() {
        let schema =
            ConfigSchema::new("app", "1.0.0").field(ConfigField::new("port", ConfigType::Integer));

        let mut values = HashMap::new();
        let result = schema.validate(&values);
        assert!(result.is_err());

        values.insert("port".to_string(), "8080".to_string());
        let result = schema.validate(&values);
        assert!(result.is_ok());
    }

    #[test]
    fn test_config_schema_validate_optional_field() {
        let schema = ConfigSchema::new("app", "1.0.0")
            .field(ConfigField::new("debug", ConfigType::Boolean).optional());

        let values = HashMap::new();
        let result = schema.validate(&values);
        assert!(result.is_ok());
    }

    #[test]
    fn test_config_schema_validate_unknown_field() {
        let schema = ConfigSchema::new("app", "1.0.0");

        let mut values = HashMap::new();
        values.insert("unknown".to_string(), "value".to_string());

        let result = schema.validate(&values);
        assert!(result.is_err());
    }

    #[test]
    fn test_config_schema_validate_integer() {
        let schema =
            ConfigSchema::new("app", "1.0.0").field(ConfigField::new("port", ConfigType::Integer));

        let mut values = HashMap::new();
        values.insert("port".to_string(), "not_a_number".to_string());

        let result = schema.validate(&values);
        assert!(result.is_err());
    }

    #[test]
    fn test_config_schema_validate_integer_range() {
        let schema = ConfigSchema::new("app", "1.0.0").field(
            ConfigField::new("port", ConfigType::Integer)
                .with_constraints(ConfigConstraint::new().with_min(1024).with_max(65535)),
        );

        let mut values = HashMap::new();
        values.insert("port".to_string(), "80".to_string());

        let result = schema.validate(&values);
        assert!(result.is_err());
    }

    #[test]
    fn test_config_schema_validate_string_length() {
        let schema = ConfigSchema::new("app", "1.0.0").field(
            ConfigField::new("name", ConfigType::String).with_constraints(
                ConfigConstraint::new()
                    .with_min_length(3)
                    .with_max_length(20),
            ),
        );

        let mut values = HashMap::new();
        values.insert("name".to_string(), "ab".to_string());

        let result = schema.validate(&values);
        assert!(result.is_err());
    }

    #[test]
    fn test_config_schema_validate_enum() {
        let schema = ConfigSchema::new("app", "1.0.0").field(
            ConfigField::new("env", ConfigType::String).with_constraints(
                ConfigConstraint::new().with_enum(vec!["dev".into(), "prod".into(), "test".into()]),
            ),
        );

        let mut values = HashMap::new();
        values.insert("env".to_string(), "invalid".to_string());

        let result = schema.validate(&values);
        assert!(result.is_err());

        values.insert("env".to_string(), "prod".to_string());
        let result = schema.validate(&values);
        assert!(result.is_ok());
    }

    #[test]
    fn test_config_schema_validate_boolean() {
        let schema =
            ConfigSchema::new("app", "1.0.0").field(ConfigField::new("debug", ConfigType::Boolean));

        let mut values = HashMap::new();
        values.insert("debug".to_string(), "yes".to_string());

        let result = schema.validate(&values);
        assert!(result.is_err());

        values.insert("debug".to_string(), "true".to_string());
        let result = schema.validate(&values);
        assert!(result.is_ok());
    }

    #[test]
    fn test_config_schema_to_json_schema() {
        let schema = ConfigSchema::new("app", "1.0.0")
            .with_description("Application configuration")
            .field(ConfigField::new("port", ConfigType::Integer).with_default("8080"));

        let json = schema.to_json_schema();
        assert!(json.contains("\"name\": \"app\""));
        assert!(json.contains("\"version\": \"1.0.0\""));
        assert!(json.contains("\"port\""));
    }

    #[test]
    fn test_config_schema_multiple_validations() {
        let schema = ConfigSchema::new("app", "1.0.0")
            .field(ConfigField::new("port", ConfigType::Integer))
            .field(ConfigField::new("host", ConfigType::String))
            .field(ConfigField::new("debug", ConfigType::Boolean).optional());

        let mut values = HashMap::new();
        values.insert("port".to_string(), "8080".to_string());
        values.insert("host".to_string(), "localhost".to_string());

        let result = schema.validate(&values);
        assert!(result.is_ok());
    }

    #[test]
    fn test_config_field_builder_chain() {
        let field = ConfigField::new("port", ConfigType::Integer)
            .with_description("Server port")
            .with_default("8080")
            .with_constraints(ConfigConstraint::new().with_min(1024).with_max(65535));

        assert_eq!(field.description, Some("Server port".to_string()));
        assert_eq!(field.default, Some("8080".to_string()));
        assert_eq!(field.constraints.min, Some(1024));
    }
}
