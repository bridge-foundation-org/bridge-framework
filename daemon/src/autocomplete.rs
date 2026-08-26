//! CLI Autocomplete - Shell completion support for Bridge CLI
//!
//! Generates shell completions for bash, zsh, fish, and powershell

// Parts of this module are forward-scaffolding: their public API is
// intentionally ahead of its call sites. Trim this allow item-by-item as the
// dead surface shrinks.
#![allow(dead_code)]

/// Shell type for autocomplete
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ShellType {
    Bash,
    Zsh,
    Fish,
    PowerShell,
}

impl ShellType {
    pub fn name(&self) -> &'static str {
        match self {
            ShellType::Bash => "bash",
            ShellType::Zsh => "zsh",
            ShellType::Fish => "fish",
            ShellType::PowerShell => "powershell",
        }
    }
}

/// CLI command for autocomplete
#[derive(Clone, Debug)]
pub struct Command {
    pub name: String,
    pub description: String,
    pub subcommands: Vec<Command>,
    pub options: Vec<CliOption>,
}

impl Command {
    pub fn new(name: impl Into<String>, description: impl Into<String>) -> Self {
        Command {
            name: name.into(),
            description: description.into(),
            subcommands: Vec::new(),
            options: Vec::new(),
        }
    }

    pub fn subcommand(mut self, cmd: Command) -> Self {
        self.subcommands.push(cmd);
        self
    }

    pub fn option(mut self, opt: CliOption) -> Self {
        self.options.push(opt);
        self
    }
}

/// CLI option/flag
#[derive(Clone, Debug)]
pub struct CliOption {
    pub short: std::option::Option<String>,
    pub long: String,
    pub description: String,
    pub takes_value: bool,
}

impl CliOption {
    pub fn new_flag(long: impl Into<String>, description: impl Into<String>) -> Self {
        CliOption {
            short: None,
            long: long.into(),
            description: description.into(),
            takes_value: false,
        }
    }

    pub fn new_value(long: impl Into<String>, description: impl Into<String>) -> Self {
        CliOption {
            short: None,
            long: long.into(),
            description: description.into(),
            takes_value: true,
        }
    }

    pub fn with_short(mut self, short: impl Into<String>) -> Self {
        self.short = Some(short.into());
        self
    }
}

/// Autocomplete generator
pub struct AutocompleteGenerator {
    commands: Vec<Command>,
}

impl AutocompleteGenerator {
    pub fn new() -> Self {
        AutocompleteGenerator {
            commands: Vec::new(),
        }
    }

    pub fn command(mut self, cmd: Command) -> Self {
        self.commands.push(cmd);
        self
    }

    /// Generate bash completion script
    pub fn bash_completion(&self) -> String {
        let mut script = String::from("_bridge_completion() {\n");
        script.push_str("  local cur prev words cword\n");
        script.push_str("  _get_comp_words_by_ref -n : cur prev words cword\n\n");

        script.push_str("  local commands=\"");
        let cmd_names: Vec<&str> = self.commands.iter().map(|c| c.name.as_str()).collect();
        script.push_str(&cmd_names.join(" "));
        script.push_str("\"\n\n");

        script.push_str("  case \"${prev}\" in\n");
        script.push_str("    *)\n");
        script.push_str("      COMPREPLY=( $(compgen -W \"${commands}\" -- ${cur}) )\n");
        script.push_str("      ;;\n");
        script.push_str("  esac\n");
        script.push_str("}\n");
        script.push_str("complete -F _bridge_completion bridge\n");

        script
    }

    /// Generate zsh completion script
    pub fn zsh_completion(&self) -> String {
        let mut script = String::from("#compdef bridge\n\n");
        script.push_str("local -a commands\n");
        script.push_str("commands=(\n");
        for cmd in &self.commands {
            script.push_str(&format!("  '{}:{}'\n", cmd.name, cmd.description));
        }
        script.push_str(")\n\n");
        script.push_str("_describe -t commands 'bridge commands' commands\n");

        script
    }

    /// Generate fish completion script
    pub fn fish_completion(&self) -> String {
        let mut script = String::new();
        for cmd in &self.commands {
            script.push_str(&format!(
                "complete -c bridge -n \"__fish_use_subcommand_from_list\" -a {} -d \"{}\"\n",
                cmd.name, cmd.description
            ));
        }
        script
    }

    /// Generate powershell completion script
    pub fn powershell_completion(&self) -> String {
        let mut script =
            String::from("Register-ArgumentCompleter -CommandName bridge -ScriptBlock {\n");
        script.push_str("  param($wordToComplete, $commandAst, $cursorPosition)\n");
        script.push_str("  $commands = @(");

        let cmd_names: Vec<String> = self
            .commands
            .iter()
            .map(|c| format!("'{}'", c.name))
            .collect();
        script.push_str(&cmd_names.join(", "));
        script.push_str(")\n\n");

        script.push_str(
            "  $commands | Where-Object { $_ -like \"$wordToComplete*\" } | ForEach-Object {\n",
        );
        script.push_str("    [System.Management.Automation.CompletionResult]::new($_, $_, 'ParameterValue', $_)\n");
        script.push_str("  }\n");
        script.push_str("}\n");

        script
    }
}

impl Default for AutocompleteGenerator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_shell_type_bash() {
        assert_eq!(ShellType::Bash.name(), "bash");
    }

    #[test]
    fn test_shell_type_zsh() {
        assert_eq!(ShellType::Zsh.name(), "zsh");
    }

    #[test]
    fn test_shell_type_fish() {
        assert_eq!(ShellType::Fish.name(), "fish");
    }

    #[test]
    fn test_shell_type_powershell() {
        assert_eq!(ShellType::PowerShell.name(), "powershell");
    }

    #[test]
    fn test_command_new() {
        let cmd = Command::new("test", "Test command");
        assert_eq!(cmd.name, "test");
        assert_eq!(cmd.description, "Test command");
        assert!(cmd.subcommands.is_empty());
    }

    #[test]
    fn test_command_with_subcommand() {
        let sub = Command::new("sub", "Subcommand");
        let cmd = Command::new("test", "Test").subcommand(sub);
        assert_eq!(cmd.subcommands.len(), 1);
    }

    #[test]
    fn test_option_flag() {
        let opt = CliOption::new_flag("verbose", "Verbose output");
        assert!(!opt.takes_value);
        assert_eq!(opt.long, "verbose");
    }

    #[test]
    fn test_option_value() {
        let opt = CliOption::new_value("output", "Output file");
        assert!(opt.takes_value);
    }

    #[test]
    fn test_option_with_short() {
        let opt = CliOption::new_flag("verbose", "Verbose").with_short("v");
        assert_eq!(opt.short, Some("v".to_string()));
    }

    #[test]
    fn test_autocomplete_generator_new() {
        let gen = AutocompleteGenerator::new();
        assert!(gen.commands.is_empty());
    }

    #[test]
    fn test_autocomplete_generator_command() {
        let cmd = Command::new("init", "Initialize");
        let gen = AutocompleteGenerator::new().command(cmd);
        assert_eq!(gen.commands.len(), 1);
    }

    #[test]
    fn test_bash_completion_generation() {
        let cmd = Command::new("ping", "Health check");
        let gen = AutocompleteGenerator::new().command(cmd);
        let script = gen.bash_completion();
        assert!(script.contains("_bridge_completion"));
        assert!(script.contains("ping"));
    }

    #[test]
    fn test_zsh_completion_generation() {
        let cmd = Command::new("run", "Run app");
        let gen = AutocompleteGenerator::new().command(cmd);
        let script = gen.zsh_completion();
        assert!(script.contains("#compdef bridge"));
        assert!(script.contains("run"));
    }

    #[test]
    fn test_fish_completion_generation() {
        let cmd = Command::new("test", "Run tests");
        let gen = AutocompleteGenerator::new().command(cmd);
        let script = gen.fish_completion();
        assert!(script.contains("complete -c bridge"));
        assert!(script.contains("test"));
    }

    #[test]
    fn test_powershell_completion_generation() {
        let cmd = Command::new("build", "Build project");
        let gen = AutocompleteGenerator::new().command(cmd);
        let script = gen.powershell_completion();
        assert!(script.contains("Register-ArgumentCompleter"));
        assert!(script.contains("build"));
    }

    #[test]
    fn test_multiple_commands() {
        let gen = AutocompleteGenerator::new()
            .command(Command::new("init", "Init"))
            .command(Command::new("run", "Run"))
            .command(Command::new("test", "Test"));

        assert_eq!(gen.commands.len(), 3);
    }

    #[test]
    fn test_command_builder_chain() {
        let opt1 = CliOption::new_flag("verbose", "Verbose").with_short("v");
        let opt2 = CliOption::new_value("config", "Config file");

        let cmd = Command::new("build", "Build").option(opt1).option(opt2);

        assert_eq!(cmd.options.len(), 2);
    }
}
