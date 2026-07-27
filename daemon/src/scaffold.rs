//! Project Scaffolding - Generate Bridge projects from templates
//!
//! Provides templates for common project types to help users get started quickly.

use std::collections::HashMap;

/// Project template type
#[derive(Clone, Debug, PartialEq, Eq, Copy)]
pub enum TemplateType {
    /// REST API service
    RestApi,
    /// Microservices architecture
    Microservices,
    /// Pub/Sub messaging
    PubSub,
    /// WebSocket real-time
    WebSocket,
    /// Minimal starter
    Minimal,
}

impl TemplateType {
    /// Get template name
    pub fn name(&self) -> &'static str {
        match self {
            TemplateType::RestApi => "rest-api",
            TemplateType::Microservices => "microservices",
            TemplateType::PubSub => "pubsub",
            TemplateType::WebSocket => "websocket",
            TemplateType::Minimal => "minimal",
        }
    }

    /// Get template description
    pub fn description(&self) -> &'static str {
        match self {
            TemplateType::RestApi => "REST API with database and authentication",
            TemplateType::Microservices => "Multiple services with inter-service communication",
            TemplateType::PubSub => "Pub/Sub messaging and event handling",
            TemplateType::WebSocket => "WebSocket support for real-time features",
            TemplateType::Minimal => "Minimal starter project",
        }
    }
}

/// Project configuration
#[derive(Clone, Debug)]
pub struct ProjectConfig {
    /// Project name
    pub name: String,
    /// Project description
    pub description: String,
    /// Template type
    pub template: TemplateType,
    /// Create database
    pub with_database: bool,
    /// Enable authentication
    pub with_auth: bool,
}

impl ProjectConfig {
    /// Create a new project config
    pub fn new(name: impl Into<String>, template: TemplateType) -> Self {
        ProjectConfig {
            name: name.into(),
            description: String::new(),
            template,
            with_database: false,
            with_auth: false,
        }
    }

    /// Set description
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = description.into();
        self
    }

    /// Enable database
    pub fn with_database(mut self) -> Self {
        self.with_database = true;
        self
    }

    /// Enable authentication
    pub fn with_auth(mut self) -> Self {
        self.with_auth = true;
        self
    }
}

/// Template generator
pub struct TemplateGenerator {
    config: ProjectConfig,
}

impl TemplateGenerator {
    /// Create a new template generator
    pub fn new(config: ProjectConfig) -> Self {
        TemplateGenerator { config }
    }

    /// Generate bridge.toml content
    pub fn bridge_toml(&self) -> String {
        let mut content = String::new();
        content.push_str("[app]\n");
        content.push_str(&format!("name = \"{}\"\n", self.config.name));
        content.push_str(&format!("description = \"{}\"\n", self.config.description));
        content.push_str("version = \"0.1.0\"\n\n");

        if self.config.with_database {
            content.push_str("[database]\n");
            content.push_str("default = \"postgres\"\n\n");
        }

        if self.config.with_auth {
            content.push_str("[auth]\n");
            content.push_str("enabled = true\n\n");
        }

        content.push_str("[server]\n");
        content.push_str("port = 8000\n");
        content
    }

    /// Generate Cargo.toml content
    pub fn cargo_toml(&self) -> String {
        let name = self.config.name.replace('-', "_");
        format!(
            "[package]\nname = \"{}\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
            name
        )
    }

    /// Generate main.rs content
    pub fn main_rs(&self) -> String {
        match self.config.template {
            TemplateType::RestApi => "fn main() { println!(\"REST API started\"); }".to_string(),
            TemplateType::Microservices => {
                "fn main() { println!(\"Microservices started\"); }".to_string()
            }
            TemplateType::PubSub => "fn main() { println!(\"PubSub app started\"); }".to_string(),
            TemplateType::WebSocket => {
                "fn main() { println!(\"WebSocket server started\"); }".to_string()
            }
            TemplateType::Minimal => "fn main() { println!(\"App started\"); }".to_string(),
        }
    }

    /// Generate README.md content
    pub fn readme(&self) -> String {
        format!(
            "# {}\n\n{}\n\n## Getting Started\n\n```bash\ncargo build\ncargo run\n```\n",
            self.config.name, self.config.description
        )
    }

    /// Generate .gitignore content
    pub fn gitignore(&self) -> String {
        "target/\nCargo.lock\n.env\n.DS_Store\n".to_string()
    }

    /// Get all generated files
    pub fn files(&self) -> HashMap<String, String> {
        let mut files = HashMap::new();
        files.insert("bridge.toml".to_string(), self.bridge_toml());
        files.insert("Cargo.toml".to_string(), self.cargo_toml());
        files.insert("src/main.rs".to_string(), self.main_rs());
        files.insert("README.md".to_string(), self.readme());
        files.insert(".gitignore".to_string(), self.gitignore());
        files
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_template_type_rest_api() {
        assert_eq!(TemplateType::RestApi.name(), "rest-api");
    }

    #[test]
    fn test_template_type_microservices() {
        assert_eq!(TemplateType::Microservices.name(), "microservices");
    }

    #[test]
    fn test_template_type_pubsub() {
        assert_eq!(TemplateType::PubSub.name(), "pubsub");
    }

    #[test]
    fn test_template_type_websocket() {
        assert_eq!(TemplateType::WebSocket.name(), "websocket");
    }

    #[test]
    fn test_template_type_minimal() {
        assert_eq!(TemplateType::Minimal.name(), "minimal");
    }

    #[test]
    fn test_template_description() {
        assert!(!TemplateType::RestApi.description().is_empty());
    }

    #[test]
    fn test_project_config_new() {
        let config = ProjectConfig::new("myapp", TemplateType::RestApi);
        assert_eq!(config.name, "myapp");
        assert!(!config.with_database);
        assert!(!config.with_auth);
    }

    #[test]
    fn test_project_config_with_database() {
        let config = ProjectConfig::new("myapp", TemplateType::RestApi).with_database();
        assert!(config.with_database);
    }

    #[test]
    fn test_project_config_with_auth() {
        let config = ProjectConfig::new("myapp", TemplateType::RestApi).with_auth();
        assert!(config.with_auth);
    }

    #[test]
    fn test_project_config_description() {
        let config = ProjectConfig::new("myapp", TemplateType::RestApi)
            .with_description("My app");
        assert_eq!(config.description, "My app");
    }

    #[test]
    fn test_generator_bridge_toml() {
        let config = ProjectConfig::new("myapp", TemplateType::RestApi);
        let gen = TemplateGenerator::new(config);
        let toml = gen.bridge_toml();

        assert!(toml.contains("name = \"myapp\""));
        assert!(toml.contains("[app]"));
    }

    #[test]
    fn test_generator_bridge_toml_with_db() {
        let config = ProjectConfig::new("myapp", TemplateType::RestApi).with_database();
        let gen = TemplateGenerator::new(config);
        let toml = gen.bridge_toml();

        assert!(toml.contains("[database]"));
    }

    #[test]
    fn test_generator_bridge_toml_with_auth() {
        let config = ProjectConfig::new("myapp", TemplateType::RestApi).with_auth();
        let gen = TemplateGenerator::new(config);
        let toml = gen.bridge_toml();

        assert!(toml.contains("[auth]"));
    }

    #[test]
    fn test_generator_cargo_toml() {
        let config = ProjectConfig::new("my-app", TemplateType::RestApi);
        let gen = TemplateGenerator::new(config);
        let cargo = gen.cargo_toml();

        assert!(cargo.contains("my_app"));
    }

    #[test]
    fn test_generator_main_rs() {
        let config = ProjectConfig::new("myapp", TemplateType::RestApi);
        let gen = TemplateGenerator::new(config);
        let main = gen.main_rs();

        assert!(!main.is_empty());
    }

    #[test]
    fn test_generator_readme() {
        let config = ProjectConfig::new("myapp", TemplateType::RestApi)
            .with_description("Test app");
        let gen = TemplateGenerator::new(config);
        let readme = gen.readme();

        assert!(readme.contains("myapp"));
        assert!(readme.contains("Test app"));
    }

    #[test]
    fn test_generator_gitignore() {
        let config = ProjectConfig::new("myapp", TemplateType::RestApi);
        let gen = TemplateGenerator::new(config);
        let gitignore = gen.gitignore();

        assert!(gitignore.contains("target/"));
    }

    #[test]
    fn test_generator_files() {
        let config = ProjectConfig::new("myapp", TemplateType::RestApi);
        let gen = TemplateGenerator::new(config);
        let files = gen.files();

        assert_eq!(files.len(), 5);
        assert!(files.contains_key("bridge.toml"));
        assert!(files.contains_key("Cargo.toml"));
        assert!(files.contains_key("src/main.rs"));
        assert!(files.contains_key("README.md"));
        assert!(files.contains_key(".gitignore"));
    }

    #[test]
    fn test_all_templates_generate_files() {
        let templates = [
            TemplateType::RestApi,
            TemplateType::Microservices,
            TemplateType::PubSub,
            TemplateType::WebSocket,
            TemplateType::Minimal,
        ];

        for template in &templates {
            let config = ProjectConfig::new("app", *template);
            let gen = TemplateGenerator::new(config);
            let files = gen.files();
            assert_eq!(files.len(), 5);
        }
    }

    #[test]
    fn test_config_builder_chain() {
        let config = ProjectConfig::new("myapp", TemplateType::RestApi)
            .with_description("My app")
            .with_database()
            .with_auth();

        assert!(config.with_database);
        assert!(config.with_auth);
        assert_eq!(config.description, "My app");
    }
}
