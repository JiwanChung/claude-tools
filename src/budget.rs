use anyhow::{Context, Result};
use clap::Subcommand;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Subcommand, Debug)]
pub enum BudgetAction {
    /// Show current token usage and budget status
    Status,

    /// Set budget limits
    Set {
        /// Daily token limit (e.g., 100k, 1m)
        #[arg(long)]
        daily: Option<String>,

        /// Weekly token limit
        #[arg(long)]
        weekly: Option<String>,

        /// Monthly token limit
        #[arg(long)]
        monthly: Option<String>,
    },

    /// Show detailed usage report
    Report {
        /// Number of days to include
        #[arg(short, long, default_value = "7")]
        days: u32,

        /// Group by: day, project, session
        #[arg(short, long, default_value = "day")]
        group_by: String,
    },

    /// Reset usage counters
    Reset {
        /// Confirm reset
        #[arg(long)]
        confirm: bool,
    },
}

#[derive(Debug, Serialize, Deserialize, Default)]
struct BudgetConfig {
    daily_limit: Option<u64>,
    weekly_limit: Option<u64>,
    monthly_limit: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct SessionMessage {
    message: Option<MessageContent>,
}

#[derive(Debug, Deserialize)]
struct MessageContent {
    usage: Option<TokenUsage>,
}

#[derive(Debug, Deserialize)]
struct TokenUsage {
    input_tokens: Option<u64>,
    output_tokens: Option<u64>,
}

#[derive(Debug, Default)]
struct UsageStats {
    total_input: u64,
    total_output: u64,
    session_count: u32,
    by_day: std::collections::HashMap<String, DayUsage>,
    by_project: std::collections::HashMap<String, u64>,
}

#[derive(Debug, Default, Clone)]
struct DayUsage {
    input: u64,
    output: u64,
}

pub fn run(action: BudgetAction) -> Result<()> {
    match action {
        BudgetAction::Status => show_status(),
        BudgetAction::Set { daily, weekly, monthly } => set_limits(daily, weekly, monthly),
        BudgetAction::Report { days, group_by } => show_report(days, &group_by),
        BudgetAction::Reset { confirm } => reset_usage(confirm),
    }
}

fn get_config_path() -> Result<PathBuf> {
    let home = std::env::var("HOME").context("HOME not set")?;
    Ok(PathBuf::from(home).join(".claude").join("budget.json"))
}

fn get_claude_projects_dir() -> Result<PathBuf> {
    let home = std::env::var("HOME").context("HOME not set")?;
    Ok(PathBuf::from(home).join(".claude").join("projects"))
}

fn load_config() -> Result<BudgetConfig> {
    let path = get_config_path()?;
    if path.exists() {
        let content = fs::read_to_string(&path)?;
        Ok(serde_json::from_str(&content)?)
    } else {
        Ok(BudgetConfig::default())
    }
}

fn save_config(config: &BudgetConfig) -> Result<()> {
    let path = get_config_path()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let content = serde_json::to_string_pretty(config)?;
    fs::write(&path, content)?;
    Ok(())
}

fn parse_token_limit(s: &str) -> Result<u64> {
    let s = s.to_lowercase().trim().to_string();

    if s.ends_with('k') {
        let num: f64 = s[..s.len()-1].parse()?;
        Ok((num * 1000.0) as u64)
    } else if s.ends_with('m') {
        let num: f64 = s[..s.len()-1].parse()?;
        Ok((num * 1_000_000.0) as u64)
    } else {
        Ok(s.parse()?)
    }
}

fn format_tokens(n: u64) -> String {
    if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1_000_000.0)
    } else if n >= 1000 {
        format!("{:.1}k", n as f64 / 1000.0)
    } else {
        format!("{}", n)
    }
}

fn calculate_usage(days: u32) -> Result<UsageStats> {
    let projects_dir = get_claude_projects_dir()?;
    let mut stats = UsageStats::default();

    if !projects_dir.exists() {
        return Ok(stats);
    }

    let cutoff = std::time::SystemTime::now()
        .checked_sub(std::time::Duration::from_secs(days as u64 * 86400))
        .unwrap_or(std::time::UNIX_EPOCH);

    for entry in fs::read_dir(&projects_dir)? {
        let entry = entry?;
        let path = entry.path();

        if !path.is_dir() {
            continue;
        }

        let project_name = path.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown")
            .to_string();

        for file_entry in fs::read_dir(&path)? {
            let file_entry = file_entry?;
            let file_path = file_entry.path();

            if !file_path.extension().map(|e| e == "jsonl").unwrap_or(false) {
                continue;
            }

            let metadata = fs::metadata(&file_path)?;
            if metadata.modified()? < cutoff {
                continue;
            }

            stats.session_count += 1;

            if let Ok(content) = fs::read_to_string(&file_path) {
                for line in content.lines() {
                    if let Ok(msg) = serde_json::from_str::<SessionMessage>(line) {
                        if let Some(message) = msg.message {
                            if let Some(usage) = message.usage {
                                let input = usage.input_tokens.unwrap_or(0);
                                let output = usage.output_tokens.unwrap_or(0);

                                stats.total_input += input;
                                stats.total_output += output;

                                *stats.by_project.entry(project_name.clone()).or_insert(0) += input + output;

                                // Group by day (using file modification time as proxy)
                                let day = get_day_string(metadata.modified()?);
                                let day_usage = stats.by_day.entry(day).or_insert_with(DayUsage::default);
                                day_usage.input += input;
                                day_usage.output += output;
                            }
                        }
                    }
                }
            }
        }
    }

    Ok(stats)
}

fn get_day_string(time: std::time::SystemTime) -> String {
    let duration = time.duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
    let days = duration.as_secs() / 86400;
    format!("day-{}", days)
}

fn show_status() -> Result<()> {
    let config = load_config()?;
    let stats = calculate_usage(30)?;

    let total = stats.total_input + stats.total_output;

    println!("Token Usage Status");
    println!("{}", "=".repeat(50));
    println!();

    println!("Current Usage (last 30 days):");
    println!("  Input tokens:  {}", format_tokens(stats.total_input));
    println!("  Output tokens: {}", format_tokens(stats.total_output));
    println!("  Total:         {}", format_tokens(total));
    println!("  Sessions:      {}", stats.session_count);
    println!();

    println!("Budget Limits:");

    if let Some(daily) = config.daily_limit {
        let daily_usage = calculate_usage(1)?.total_input + calculate_usage(1)?.total_output;
        let pct = (daily_usage as f64 / daily as f64 * 100.0).min(100.0);
        let status = if daily_usage > daily { "\x1b[31mEXCEEDED\x1b[0m" } else { "\x1b[32mOK\x1b[0m" };
        println!("  Daily:   {}/{} ({:.0}%) {}", format_tokens(daily_usage), format_tokens(daily), pct, status);
    } else {
        println!("  Daily:   not set");
    }

    if let Some(weekly) = config.weekly_limit {
        let weekly_usage = calculate_usage(7)?.total_input + calculate_usage(7)?.total_output;
        let pct = (weekly_usage as f64 / weekly as f64 * 100.0).min(100.0);
        let status = if weekly_usage > weekly { "\x1b[31mEXCEEDED\x1b[0m" } else { "\x1b[32mOK\x1b[0m" };
        println!("  Weekly:  {}/{} ({:.0}%) {}", format_tokens(weekly_usage), format_tokens(weekly), pct, status);
    } else {
        println!("  Weekly:  not set");
    }

    if let Some(monthly) = config.monthly_limit {
        let pct = (total as f64 / monthly as f64 * 100.0).min(100.0);
        let status = if total > monthly { "\x1b[31mEXCEEDED\x1b[0m" } else { "\x1b[32mOK\x1b[0m" };
        println!("  Monthly: {}/{} ({:.0}%) {}", format_tokens(total), format_tokens(monthly), pct, status);
    } else {
        println!("  Monthly: not set");
    }

    println!();
    println!("Use 'claude-tools budget set --daily 100k' to set limits");
    println!("Use 'claude-tools budget report' for detailed breakdown");

    Ok(())
}

fn set_limits(daily: Option<String>, weekly: Option<String>, monthly: Option<String>) -> Result<()> {
    let mut config = load_config()?;

    if let Some(d) = daily {
        config.daily_limit = Some(parse_token_limit(&d)?);
        println!("Daily limit set to: {}", format_tokens(config.daily_limit.unwrap()));
    }

    if let Some(w) = weekly {
        config.weekly_limit = Some(parse_token_limit(&w)?);
        println!("Weekly limit set to: {}", format_tokens(config.weekly_limit.unwrap()));
    }

    if let Some(m) = monthly {
        config.monthly_limit = Some(parse_token_limit(&m)?);
        println!("Monthly limit set to: {}", format_tokens(config.monthly_limit.unwrap()));
    }

    save_config(&config)?;
    println!("\nConfig saved to: {}", get_config_path()?.display());

    Ok(())
}

fn show_report(days: u32, group_by: &str) -> Result<()> {
    let stats = calculate_usage(days)?;

    println!("Usage Report (last {} days)", days);
    println!("{}", "=".repeat(50));
    println!();

    match group_by {
        "project" => {
            println!("{:<30} {}", "Project", "Tokens");
            println!("{}", "-".repeat(50));

            let mut projects: Vec<_> = stats.by_project.iter().collect();
            projects.sort_by(|a, b| b.1.cmp(a.1));

            for (project, tokens) in projects {
                println!("{:<30} {}", project, format_tokens(*tokens));
            }
        }
        "day" => {
            println!("{:<15} {:<15} {:<15} {}", "Day", "Input", "Output", "Total");
            println!("{}", "-".repeat(60));

            let mut days: Vec<_> = stats.by_day.iter().collect();
            days.sort_by(|a, b| a.0.cmp(b.0));

            for (day, usage) in days {
                let total = usage.input + usage.output;
                println!(
                    "{:<15} {:<15} {:<15} {}",
                    day,
                    format_tokens(usage.input),
                    format_tokens(usage.output),
                    format_tokens(total)
                );
            }
        }
        _ => {
            println!("Unknown grouping: {}", group_by);
        }
    }

    println!();
    println!("Total: {} tokens across {} sessions",
        format_tokens(stats.total_input + stats.total_output),
        stats.session_count
    );

    Ok(())
}

fn reset_usage(confirm: bool) -> Result<()> {
    if !confirm {
        println!("This will reset all usage tracking data.");
        println!("Run with --confirm to proceed.");
        return Ok(());
    }

    let config = BudgetConfig::default();
    save_config(&config)?;

    println!("Budget configuration reset.");
    println!("Note: Historical usage in JSONL files is not deleted.");

    Ok(())
}
