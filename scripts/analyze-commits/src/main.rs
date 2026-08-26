#!/usr/bin/env rust-script
//! Bridge Commit Analyzer
//!
//! Parses e-commits/commits.json and categorizes Encore commits by feature area.
//! Generates a prioritized implementation roadmap for Bridge Framework.
//!
//! Usage: cargo run --bin analyze-commits

use std::collections::HashMap;
use std::fs;
use std::path::Path;

fn main() {
    println!("🔍 Bridge Framework - Commit Analyzer");
    println!("==========================================\n");

    // Read commits.json
    let commits_path = "e-commits/commits.json";
    println!("📖 Reading commits from: {}", commits_path);

    if !Path::new(commits_path).exists() {
        eprintln!("❌ Error: {} not found", commits_path);
        eprintln!("   Make sure you're running from the project root.");
        std::process::exit(1);
    }

    let commits_json = match fs::read_to_string(commits_path) {
        Ok(content) => content,
        Err(e) => {
            eprintln!("❌ Failed to read commits.json: {}", e);
            std::process::exit(1);
        }
    };

    println!("✓ Loaded {} bytes\n", commits_json.len());

    // Parse and categorize commits
    println!("🏗️  Categorizing commits by feature area...");
    let categories = categorize_commits(&commits_json);

    println!("\n📊 Commit Categories:");
    println!("─────────────────────────────────────────");

    let mut total = 0;
    for (category, count) in &categories {
        println!("  {:30} {:>4} commits", category, count);
        total += count;
    }
    println!("─────────────────────────────────────────");
    println!("  {:30} {:>4} total\n", "TOTAL", total);

    // Generate implementation priority
    println!("🎯 Implementation Priority:");
    println!("─────────────────────────────────────────");
    let priorities = generate_priorities(&categories);
    for (rank, (category, _)) in priorities.iter().enumerate() {
        let priority_icon = match rank {
            0..=2 => "🔥",
            3..=5 => "⚡",
            6..=8 => "📋",
            _ => "💡",
        };
        println!("  {}. {} {}", rank + 1, priority_icon, category);
    }

    println!("\n✅ Analysis complete!");
    println!("   Next: Implement features in priority order");
    println!("   Track progress in: IMPLEMENTATION_TRACKER.md");
}

/// Categorize commits by analyzing subject/body text for keywords
fn categorize_commits(json: &str) -> HashMap<String, usize> {
    let mut categories = HashMap::new();

    // Feature keywords to search for
    let patterns = vec![
        (
            "Core Runtime",
            vec!["runtime", "runtimes-core", "metrics", "tracing", "trace"],
        ),
        (
            "TypeScript Runtime",
            vec!["runtimes-js", "typescript", "tsparser", "encore-ts"],
        ),
        (
            "Authentication",
            vec!["auth", "authentication", "jwt", "session", "oauth"],
        ),
        (
            "Object Storage",
            vec!["storage", "bucket", "s3", "object-storage"],
        ),
        (
            "Pub/Sub",
            vec!["pubsub", "pub-sub", "topic", "subscription", "message"],
        ),
        ("Caching", vec!["cache", "redis", "mget", "mset"]),
        ("Secrets", vec!["secret", "vault", "jit"]),
        (
            "Infrastructure",
            vec!["infra", "config", "tls", "database", "sqldb"],
        ),
        ("Testing", vec!["test", "e2e", "mock"]),
        (
            "Documentation",
            vec!["docs", "documentation", "readme", "tutorial"],
        ),
        ("CLI", vec!["cli", "command", "daemon"]),
        ("Streaming", vec!["stream", "websocket", "sse", "ws"]),
        ("Build/Deploy", vec!["build", "deploy", "docker", "eject"]),
        ("Parser", vec!["parser", "compiler", "codegen"]),
        ("MCP/AI", vec!["mcp", "llm", "ai", "cursor"]),
        ("Database", vec!["db", "migration", "postgres", "sqldb"]),
        ("Client Gen", vec!["clientgen", "client-gen", "openapi"]),
        ("Gateway", vec!["gateway", "pingora", "proxy"]),
        ("Middleware", vec!["middleware"]),
        ("Validation", vec!["validation", "validate"]),
        ("Other", vec![]), // catch-all
    ];

    // Simple line-by-line analysis (since we're avoiding external JSON parsers)
    // Look for "subject": "..." patterns
    for line in json.lines() {
        let line_lower = line.to_lowercase();

        if line_lower.contains("\"subject\"") || line_lower.contains("\"body\"") {
            for (category, keywords) in &patterns {
                for keyword in keywords {
                    if line_lower.contains(keyword) {
                        *categories.entry(category.to_string()).or_insert(0) += 1;
                        break; // Only count once per commit per category
                    }
                }
            }
        }
    }

    // Ensure all categories exist
    for (category, _) in &patterns {
        categories.entry(category.to_string()).or_insert(0);
    }

    categories
}

/// Generate implementation priority based on category importance
fn generate_priorities(categories: &HashMap<String, usize>) -> Vec<(String, usize)> {
    // Priority weights (higher = more important)
    let weights = vec![
        ("Core Runtime", 1000),
        ("TypeScript Runtime", 950),
        ("Authentication", 900),
        ("Database", 900),
        ("Infrastructure", 850),
        ("Testing", 850),
        ("CLI", 800),
        ("Parser", 800),
        ("Streaming", 750),
        ("Object Storage", 750),
        ("Pub/Sub", 750),
        ("Caching", 700),
        ("Client Gen", 700),
        ("Gateway", 650),
        ("Middleware", 650),
        ("Validation", 650),
        ("Documentation", 600),
        ("Secrets", 550),
        ("Build/Deploy", 500),
        ("MCP/AI", 400),
        ("Other", 100),
    ];

    let mut priority_list: Vec<(String, usize)> = categories
        .iter()
        .map(|(cat, count)| {
            let weight = weights
                .iter()
                .find(|(name, _)| name == cat)
                .map(|(_, w)| w)
                .unwrap_or(&0);
            (cat.clone(), *count * weight)
        })
        .collect();

    priority_list.sort_by_key(|&(_, weight)| std::cmp::Reverse(weight));
    priority_list
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_categorize_sample() {
        let sample = r#"{"subject": "runtimes-core: add metrics support"}"#;
        let categories = categorize_commits(sample);
        assert!(categories.get("Core Runtime").unwrap_or(&0) > &0);
    }

    #[test]
    fn test_priority_generation() {
        let mut categories = HashMap::new();
        categories.insert("Core Runtime".to_string(), 100);
        categories.insert("Documentation".to_string(), 50);

        let priorities = generate_priorities(&categories);
        assert_eq!(priorities[0].0, "Core Runtime");
    }
}
