//! Go Client Code Generation
//!
//! Generates type-safe Go clients from service definitions

// Parts of this module are forward-scaffolding: their public API is
// intentionally ahead of its call sites. Trim this allow item-by-item as the
// dead surface shrinks.
#![allow(dead_code)]

use std::collections::HashMap;

/// Go type mapping for Bridge types
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GoType {
    String,
    Int,
    Float,
    Bool,
    Bytes,
    Time,
    Custom(String),
    Array(Box<GoType>),
    Map(Box<GoType>),
    Optional(Box<GoType>),
}

impl GoType {
    /// Convert to Go type string
    pub fn to_go(&self) -> String {
        match self {
            GoType::String => "string".to_string(),
            GoType::Int => "int64".to_string(),
            GoType::Float => "float64".to_string(),
            GoType::Bool => "bool".to_string(),
            GoType::Bytes => "[]byte".to_string(),
            GoType::Time => "time.Time".to_string(),
            GoType::Custom(name) => name.clone(),
            GoType::Array(inner) => format!("[]{}", inner.to_go()),
            GoType::Map(inner) => format!("map[string]{}", inner.to_go()),
            GoType::Optional(inner) => format!("*{}", inner.to_go()),
        }
    }
}

/// Go struct field
#[derive(Clone, Debug)]
pub struct GoField {
    pub name: String,
    pub go_type: GoType,
    pub json_tag: String,
}

impl GoField {
    /// Create new field
    pub fn new(name: impl Into<String>, go_type: GoType) -> Self {
        let name_str = name.into();
        let json_tag = Self::to_snake_case(&name_str);
        GoField {
            name: Self::capitalize(&name_str),
            go_type,
            json_tag,
        }
    }

    /// Capitalize first letter
    fn capitalize(s: &str) -> String {
        let mut chars = s.chars();
        match chars.next() {
            None => String::new(),
            Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        }
    }

    /// Convert to snake_case
    fn to_snake_case(s: &str) -> String {
        let mut result = String::new();
        for (i, c) in s.chars().enumerate() {
            if i > 0 && c.is_uppercase() {
                result.push('_');
            }
            result.push(c.to_lowercase().next().unwrap_or(c));
        }
        result
    }

    /// Generate field declaration
    pub fn to_go(&self) -> String {
        format!(
            "\t{} {} `json:\"{}\"`",
            self.name,
            self.go_type.to_go(),
            self.json_tag
        )
    }
}

/// Go struct definition
#[derive(Clone, Debug)]
pub struct GoStruct {
    pub name: String,
    pub fields: Vec<GoField>,
    pub doc: String,
}

impl GoStruct {
    /// Create new struct
    pub fn new(name: impl Into<String>) -> Self {
        GoStruct {
            name: name.into(),
            fields: Vec::new(),
            doc: String::new(),
        }
    }

    /// Add field
    pub fn add_field(mut self, field: GoField) -> Self {
        self.fields.push(field);
        self
    }

    /// Set documentation
    pub fn with_doc(mut self, doc: impl Into<String>) -> Self {
        self.doc = doc.into();
        self
    }

    /// Generate Go struct code
    pub fn to_go(&self) -> String {
        let mut code = String::new();
        if !self.doc.is_empty() {
            code.push_str(&format!("// {}\n", self.doc));
        }
        code.push_str(&format!("type {} struct {{\n", self.name));
        for field in &self.fields {
            code.push_str(&format!("{}\n", field.to_go()));
        }
        code.push_str("}\n");
        code
    }
}

/// Go function parameter
#[derive(Clone, Debug)]
pub struct GoParam {
    pub name: String,
    pub go_type: GoType,
}

impl GoParam {
    /// Create new parameter
    pub fn new(name: impl Into<String>, go_type: GoType) -> Self {
        GoParam {
            name: name.into(),
            go_type,
        }
    }

    /// Generate parameter string
    pub fn to_go(&self) -> String {
        format!("{} {}", self.name, self.go_type.to_go())
    }
}

/// Go method definition
#[derive(Clone, Debug)]
pub struct GoMethod {
    pub name: String,
    pub receiver: String,
    pub params: Vec<GoParam>,
    pub return_type: Option<GoType>,
    pub body: String,
    pub doc: String,
}

impl GoMethod {
    /// Create new method
    pub fn new(name: impl Into<String>, receiver: impl Into<String>) -> Self {
        GoMethod {
            name: name.into(),
            receiver: receiver.into(),
            params: Vec::new(),
            return_type: None,
            body: String::new(),
            doc: String::new(),
        }
    }

    /// Add parameter
    pub fn add_param(mut self, param: GoParam) -> Self {
        self.params.push(param);
        self
    }

    /// Set return type
    pub fn with_return(mut self, go_type: GoType) -> Self {
        self.return_type = Some(go_type);
        self
    }

    /// Set body
    pub fn with_body(mut self, body: impl Into<String>) -> Self {
        self.body = body.into();
        self
    }

    /// Set documentation
    pub fn with_doc(mut self, doc: impl Into<String>) -> Self {
        self.doc = doc.into();
        self
    }

    /// Generate Go method code
    pub fn to_go(&self) -> String {
        let mut code = String::new();
        if !self.doc.is_empty() {
            code.push_str(&format!("// {}\n", self.doc));
        }

        let params = self
            .params
            .iter()
            .map(|p| p.to_go())
            .collect::<Vec<_>>()
            .join(", ");

        let return_type = self
            .return_type
            .as_ref()
            .map(|t| format!(" {}", t.to_go()))
            .unwrap_or_default();

        code.push_str(&format!(
            "func ({} *{}) {}({}){}",
            self.receiver.chars().next().unwrap_or('c'),
            self.receiver,
            self.name,
            params,
            return_type
        ));

        if !self.body.is_empty() {
            code.push_str(" {\n");
            code.push_str(&self.body);
            code.push_str("}\n");
        } else {
            code.push('\n');
        }

        code
    }
}

/// Go client generator
pub struct GoClientGenerator {
    package: String,
    imports: Vec<String>,
    structs: HashMap<String, GoStruct>,
    methods: Vec<GoMethod>,
}

impl GoClientGenerator {
    /// Create new generator
    pub fn new(package: impl Into<String>) -> Self {
        let mut imports = vec![
            "encoding/json".to_string(),
            "fmt".to_string(),
            "io".to_string(),
            "net/http".to_string(),
            "time".to_string(),
        ];
        imports.sort();

        GoClientGenerator {
            package: package.into(),
            imports,
            structs: HashMap::new(),
            methods: Vec::new(),
        }
    }

    /// Add struct
    pub fn add_struct(mut self, s: GoStruct) -> Self {
        self.structs.insert(s.name.clone(), s);
        self
    }

    /// Add method
    pub fn add_method(mut self, method: GoMethod) -> Self {
        self.methods.push(method);
        self
    }

    /// Add custom import
    pub fn add_import(mut self, import: impl Into<String>) -> Self {
        let imp = import.into();
        if !self.imports.contains(&imp) {
            self.imports.push(imp);
            self.imports.sort();
        }
        self
    }

    /// Generate complete Go client code
    pub fn generate(&self) -> String {
        let mut code = String::new();

        // Package and imports
        code.push_str(&format!("package {}\n\n", self.package));

        if !self.imports.is_empty() {
            code.push_str("import (\n");
            for imp in &self.imports {
                code.push_str(&format!("\t\"{}\"\n", imp));
            }
            code.push_str(")\n\n");
        }

        // Structs
        for s in self.structs.values() {
            code.push_str(&s.to_go());
            code.push('\n');
        }

        // Methods
        for method in &self.methods {
            code.push_str(&method.to_go());
            code.push('\n');
        }

        code
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_go_type_string() {
        assert_eq!(GoType::String.to_go(), "string");
        assert_eq!(GoType::Int.to_go(), "int64");
        assert_eq!(GoType::Bool.to_go(), "bool");
    }

    #[test]
    fn test_go_type_complex() {
        let array_type = GoType::Array(Box::new(GoType::String));
        assert_eq!(array_type.to_go(), "[]string");

        let optional = GoType::Optional(Box::new(GoType::Int));
        assert_eq!(optional.to_go(), "*int64");

        let map_type = GoType::Map(Box::new(GoType::String));
        assert_eq!(map_type.to_go(), "map[string]string");
    }

    #[test]
    fn test_go_field_new() {
        let field = GoField::new("userName", GoType::String);
        assert_eq!(field.name, "UserName");
        assert_eq!(field.json_tag, "user_name");
    }

    #[test]
    fn test_go_field_to_go() {
        let field = GoField::new("email", GoType::String);
        let code = field.to_go();
        assert!(code.contains("Email"));
        assert!(code.contains("string"));
        assert!(code.contains("json"));
    }

    #[test]
    fn test_go_struct_new() {
        let s = GoStruct::new("User");
        assert_eq!(s.name, "User");
        assert_eq!(s.fields.len(), 0);
    }

    #[test]
    fn test_go_struct_with_fields() {
        let s = GoStruct::new("User")
            .add_field(GoField::new("id", GoType::Int))
            .add_field(GoField::new("name", GoType::String));

        assert_eq!(s.fields.len(), 2);
    }

    #[test]
    fn test_go_struct_to_go() {
        let s = GoStruct::new("User")
            .with_doc("User struct")
            .add_field(GoField::new("id", GoType::Int));

        let code = s.to_go();
        assert!(code.contains("type User struct"));
        assert!(code.contains("User struct"));
    }

    #[test]
    fn test_go_param() {
        let param = GoParam::new("ctx", GoType::Custom("context.Context".to_string()));
        assert_eq!(param.to_go(), "ctx context.Context");
    }

    #[test]
    fn test_go_method_new() {
        let method = GoMethod::new("GetUser", "Client");
        assert_eq!(method.name, "GetUser");
        assert_eq!(method.receiver, "Client");
    }

    #[test]
    fn test_go_method_with_params() {
        let method = GoMethod::new("GetUser", "Client")
            .add_param(GoParam::new("id", GoType::Int))
            .with_return(GoType::Custom("*User".to_string()));

        assert_eq!(method.params.len(), 1);
        assert!(method.return_type.is_some());
    }

    #[test]
    fn test_go_method_to_go() {
        let method = GoMethod::new("GetUser", "Client")
            .with_doc("Get user by ID")
            .add_param(GoParam::new("id", GoType::Int))
            .with_return(GoType::Custom("*User".to_string()))
            .with_body("return &User{}");

        let code = method.to_go();
        assert!(code.contains("func"));
        assert!(code.contains("GetUser"));
        assert!(code.contains("return &User{}"));
    }

    #[test]
    fn test_go_client_generator_new() {
        let gen = GoClientGenerator::new("client");
        assert_eq!(gen.package, "client");
        assert!(!gen.imports.is_empty());
    }

    #[test]
    fn test_go_client_generator_add_struct() {
        let s = GoStruct::new("User").add_field(GoField::new("id", GoType::Int));
        let gen = GoClientGenerator::new("client").add_struct(s);
        assert_eq!(gen.structs.len(), 1);
    }

    #[test]
    fn test_go_client_generator_add_method() {
        let method = GoMethod::new("GetUser", "Client");
        let gen = GoClientGenerator::new("client").add_method(method);
        assert_eq!(gen.methods.len(), 1);
    }

    #[test]
    fn test_go_client_generator_add_import() {
        let gen = GoClientGenerator::new("client").add_import("github.com/user/pkg");
        assert!(gen.imports.contains(&"github.com/user/pkg".to_string()));
    }

    #[test]
    fn test_go_client_generator_generate() {
        let user_struct = GoStruct::new("User")
            .with_doc("User represents a user")
            .add_field(GoField::new("id", GoType::Int))
            .add_field(GoField::new("name", GoType::String));

        let gen = GoClientGenerator::new("client")
            .add_struct(user_struct)
            .add_import("github.com/user/sdk");

        let code = gen.generate();
        assert!(code.contains("package client"));
        assert!(code.contains("import"));
        assert!(code.contains("type User struct"));
        assert!(code.contains("github.com/user/sdk"));
    }

    #[test]
    fn test_go_client_full_example() {
        let user_struct = GoStruct::new("User")
            .add_field(GoField::new("id", GoType::Int))
            .add_field(GoField::new("email", GoType::String));

        let get_method = GoMethod::new("GetUser", "Client")
            .with_doc("GetUser retrieves a user by ID")
            .add_param(GoParam::new("id", GoType::Int))
            .with_return(GoType::Custom("*User".to_string()))
            .with_body("// implementation");

        let gen = GoClientGenerator::new("bridge")
            .add_struct(user_struct)
            .add_method(get_method);

        let code = gen.generate();
        assert!(code.contains("package bridge"));
        assert!(code.contains("type User struct"));
        assert!(code.contains("GetUser"));
    }
}
