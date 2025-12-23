use anyhow::{Context, Result};
use clap::Subcommand;
use std::fs;
use std::path::PathBuf;

#[derive(Subcommand, Debug)]
pub enum SyncAction {
    /// Push CLAUDE.md from source to target projects
    Push {
        /// Source CLAUDE.md file or directory
        #[arg(short, long)]
        source: Option<PathBuf>,

        /// Target directories (supports glob patterns)
        targets: Vec<String>,

        /// Merge strategy: prepend, append, replace
        #[arg(short = 'm', long, default_value = "prepend")]
        strategy: String,

        /// Dry run - show what would be done
        #[arg(long)]
        dry_run: bool,
    },

    /// Show sync status of CLAUDE.md across projects
    Status {
        /// Directories to check (supports glob patterns)
        paths: Vec<String>,
    },

    /// Show diff between source and target CLAUDE.md
    Diff {
        /// Source CLAUDE.md file
        source: PathBuf,

        /// Target CLAUDE.md file
        target: PathBuf,
    },

    /// Initialize a master CLAUDE.md template
    Init {
        /// Output path
        #[arg(short, long, default_value = "~/.claude/CLAUDE.md")]
        output: String,
    },
}

pub fn run(action: SyncAction) -> Result<()> {
    match action {
        SyncAction::Push { source, targets, strategy, dry_run } => {
            push_claude_md(source, targets, &strategy, dry_run)
        }
        SyncAction::Status { paths } => show_status(paths),
        SyncAction::Diff { source, target } => show_diff(&source, &target),
        SyncAction::Init { output } => init_template(&output),
    }
}

fn get_default_source() -> Result<PathBuf> {
    let home = std::env::var("HOME").context("HOME not set")?;
    Ok(PathBuf::from(home).join(".claude").join("CLAUDE.md"))
}

fn expand_path(path: &str) -> PathBuf {
    if path.starts_with("~/") {
        if let Ok(home) = std::env::var("HOME") {
            return PathBuf::from(home).join(&path[2..]);
        }
    }
    PathBuf::from(path)
}

fn expand_glob(pattern: &str) -> Vec<PathBuf> {
    let expanded = expand_path(pattern);

    // Simple glob expansion for * patterns
    if pattern.contains('*') {
        if let Some(parent) = expanded.parent() {
            if parent.exists() {
                if let Ok(entries) = fs::read_dir(parent) {
                    return entries
                        .filter_map(|e| e.ok())
                        .map(|e| e.path())
                        .filter(|p| p.is_dir())
                        .collect();
                }
            }
        }
    }

    vec![expanded]
}

fn push_claude_md(
    source: Option<PathBuf>,
    targets: Vec<String>,
    strategy: &str,
    dry_run: bool,
) -> Result<()> {
    let source_path = source.unwrap_or_else(|| get_default_source().unwrap_or_default());

    if !source_path.exists() {
        anyhow::bail!("Source file not found: {}", source_path.display());
    }

    let source_content = fs::read_to_string(&source_path)
        .context("Failed to read source CLAUDE.md")?;

    println!("Source: {}", source_path.display());
    println!("Strategy: {}", strategy);
    println!();

    let mut success_count = 0;
    let mut skip_count = 0;

    for target_pattern in &targets {
        for target_dir in expand_glob(target_pattern) {
            if !target_dir.is_dir() {
                continue;
            }

            let target_file = target_dir.join("CLAUDE.md");
            let action = if target_file.exists() {
                match strategy {
                    "replace" => "replace",
                    "append" => "append to",
                    "prepend" => "prepend to",
                    _ => "replace",
                }
            } else {
                "create"
            };

            println!("  {} {}", action, target_file.display());

            if dry_run {
                skip_count += 1;
                continue;
            }

            let new_content = if target_file.exists() {
                let existing = fs::read_to_string(&target_file).unwrap_or_default();
                match strategy {
                    "append" => format!("{}\n\n{}", existing, source_content),
                    "prepend" => format!("{}\n\n{}", source_content, existing),
                    _ => source_content.clone(),
                }
            } else {
                source_content.clone()
            };

            fs::write(&target_file, new_content)
                .with_context(|| format!("Failed to write {}", target_file.display()))?;

            success_count += 1;
        }
    }

    println!();
    if dry_run {
        println!("Dry run: {} files would be modified", skip_count);
    } else {
        println!("Updated {} files", success_count);
    }

    Ok(())
}

fn show_status(paths: Vec<String>) -> Result<()> {
    let source_path = get_default_source()?;
    let source_hash = if source_path.exists() {
        Some(hash_file(&source_path)?)
    } else {
        None
    };

    println!("Master: {}", source_path.display());
    if source_hash.is_some() {
        println!("Status: exists");
    } else {
        println!("Status: not found (run 'claude-tools sync init' to create)");
    }
    println!();

    let search_paths = if paths.is_empty() {
        // Default to common project locations
        vec!["~/projects/*".to_string(), "~/code/*".to_string()]
    } else {
        paths
    };

    println!("{:<40} {:<10} {}", "Project", "Status", "Match");
    println!("{}", "-".repeat(70));

    for pattern in &search_paths {
        for dir in expand_glob(pattern) {
            if !dir.is_dir() {
                continue;
            }

            let claude_file = dir.join("CLAUDE.md");
            let project_name = dir.file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("?");

            if claude_file.exists() {
                let target_hash = hash_file(&claude_file)?;
                let matches = source_hash.as_ref().map(|h| h == &target_hash).unwrap_or(false);
                let match_str = if matches { "✓ synced" } else { "✗ differs" };
                let status_color = if matches { "\x1b[32m" } else { "\x1b[33m" };

                println!(
                    "{:<40} {}exists\x1b[0m   {}",
                    project_name,
                    status_color,
                    match_str
                );
            } else {
                println!(
                    "{:<40} \x1b[90mmissing\x1b[0m  -",
                    project_name
                );
            }
        }
    }

    Ok(())
}

fn show_diff(source: &PathBuf, target: &PathBuf) -> Result<()> {
    if !source.exists() {
        anyhow::bail!("Source not found: {}", source.display());
    }
    if !target.exists() {
        anyhow::bail!("Target not found: {}", target.display());
    }

    let source_content = fs::read_to_string(source)?;
    let target_content = fs::read_to_string(target)?;

    if source_content == target_content {
        println!("Files are identical");
        return Ok(());
    }

    println!("--- {}", source.display());
    println!("+++ {}", target.display());
    println!();

    // Simple line-by-line diff
    let source_lines: Vec<&str> = source_content.lines().collect();
    let target_lines: Vec<&str> = target_content.lines().collect();

    let max_lines = source_lines.len().max(target_lines.len());

    for i in 0..max_lines {
        let source_line = source_lines.get(i).map(|s| *s);
        let target_line = target_lines.get(i).map(|s| *s);

        match (source_line, target_line) {
            (Some(s), Some(t)) if s == t => {
                println!("  {}", s);
            }
            (Some(s), Some(t)) => {
                println!("\x1b[31m- {}\x1b[0m", s);
                println!("\x1b[32m+ {}\x1b[0m", t);
            }
            (Some(s), None) => {
                println!("\x1b[31m- {}\x1b[0m", s);
            }
            (None, Some(t)) => {
                println!("\x1b[32m+ {}\x1b[0m", t);
            }
            (None, None) => {}
        }
    }

    Ok(())
}

fn init_template(output: &str) -> Result<()> {
    let output_path = expand_path(output);

    if output_path.exists() {
        anyhow::bail!("File already exists: {}", output_path.display());
    }

    // Create parent directory if needed
    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent)?;
    }

    let template = r#"# CLAUDE.md - Project Guidelines

## Overview
Brief description of this project.

## Architecture
Key architectural decisions and patterns used.

## Development
- Build: `cargo build`
- Test: `cargo test`
- Lint: `cargo clippy`

## Conventions
- Code style guidelines
- Naming conventions
- File organization

## Common Tasks
Frequently performed operations and how to do them.
"#;

    fs::write(&output_path, template)?;

    println!("Created template at: {}", output_path.display());
    println!();
    println!("Edit this file, then use 'claude-tools sync push' to distribute to projects.");

    Ok(())
}

fn hash_file(path: &PathBuf) -> Result<u64> {
    use std::hash::{Hash, Hasher};
    use std::collections::hash_map::DefaultHasher;

    let content = fs::read_to_string(path)?;
    let mut hasher = DefaultHasher::new();
    content.hash(&mut hasher);
    Ok(hasher.finish())
}
