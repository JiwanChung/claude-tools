use anyhow::{Context, Result};
use clap::Subcommand;
use serde::Deserialize;
use std::fs;
use std::path::PathBuf;

#[derive(Subcommand, Debug)]
pub enum ResumeAction {
    /// List recent Claude Code sessions
    List {
        /// Number of sessions to show
        #[arg(short, long, default_value = "10")]
        limit: usize,
    },

    /// Show details of a specific session
    Show {
        /// Session ID or index from list
        session: String,
    },

    /// Open terminal in the session's project directory
    Open {
        /// Session ID or index from list
        session: String,
    },
}

#[derive(Debug, Deserialize)]
struct SessionMessage {
    message: Option<MessageContent>,
}

#[derive(Debug, Deserialize)]
struct MessageContent {
    role: Option<String>,
    content: Option<serde_json::Value>,
}

#[derive(Debug)]
pub struct Session {
    pub id: String,
    pub path: PathBuf,
    pub project_path: String,
    pub modified: std::time::SystemTime,
    pub message_count: usize,
    pub last_prompt: Option<String>,
}

pub fn run(action: ResumeAction) -> Result<()> {
    match action {
        ResumeAction::List { limit } => list_sessions(limit),
        ResumeAction::Show { session } => show_session(&session),
        ResumeAction::Open { session } => open_session(&session),
    }
}

fn get_claude_projects_dir() -> Result<PathBuf> {
    let home = std::env::var("HOME").context("HOME not set")?;
    Ok(PathBuf::from(home).join(".claude").join("projects"))
}

fn find_sessions(limit: usize) -> Result<Vec<Session>> {
    let projects_dir = get_claude_projects_dir()?;

    if !projects_dir.exists() {
        return Ok(Vec::new());
    }

    let mut sessions: Vec<Session> = Vec::new();

    // Walk through project directories
    for entry in fs::read_dir(&projects_dir)? {
        let entry = entry?;
        let path = entry.path();

        if !path.is_dir() {
            continue;
        }

        // Look for JSONL files in this project dir
        for file_entry in fs::read_dir(&path)? {
            let file_entry = file_entry?;
            let file_path = file_entry.path();

            if file_path.extension().map(|e| e == "jsonl").unwrap_or(false) {
                if let Ok(session) = parse_session_file(&file_path) {
                    sessions.push(session);
                }
            }
        }
    }

    // Sort by modification time (newest first)
    sessions.sort_by(|a, b| b.modified.cmp(&a.modified));
    sessions.truncate(limit);

    Ok(sessions)
}

fn parse_session_file(path: &PathBuf) -> Result<Session> {
    let metadata = fs::metadata(path)?;
    let modified = metadata.modified()?;

    let content = fs::read_to_string(path)?;
    let lines: Vec<&str> = content.lines().collect();

    let mut message_count = 0;
    let mut last_prompt = None;
    let mut project_path = String::new();

    for line in &lines {
        if let Ok(msg) = serde_json::from_str::<SessionMessage>(line) {
            message_count += 1;

            // Extract project path from first message if available
            if project_path.is_empty() {
                if let Some(ref content) = msg.message {
                    if content.role.as_deref() == Some("user") {
                        // Try to extract cwd from the message
                        if let Some(serde_json::Value::String(s)) = &content.content {
                            if s.contains("cwd:") {
                                if let Some(cwd) = s.split("cwd:").nth(1) {
                                    project_path = cwd.trim().split('\n').next().unwrap_or("").to_string();
                                }
                            }
                        }
                    }
                }
            }

            // Get last user prompt
            if let Some(ref content) = msg.message {
                if content.role.as_deref() == Some("user") {
                    if let Some(serde_json::Value::String(s)) = &content.content {
                        last_prompt = Some(s.chars().take(80).collect());
                    }
                }
            }
        }
    }

    // Use parent directory name as project path if not found
    if project_path.is_empty() {
        if let Some(parent) = path.parent() {
            project_path = parent.file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("unknown")
                .to_string();
        }
    }

    let id = path.file_stem()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown")
        .to_string();

    Ok(Session {
        id,
        path: path.clone(),
        project_path,
        modified,
        message_count,
        last_prompt,
    })
}

fn list_sessions(limit: usize) -> Result<()> {
    let sessions = find_sessions(limit)?;

    if sessions.is_empty() {
        println!("No Claude Code sessions found.");
        println!("Sessions are stored in ~/.claude/projects/");
        return Ok(());
    }

    println!("{:<4} {:<20} {:<30} {}", "#", "Project", "Last Modified", "Messages");
    println!("{}", "-".repeat(70));

    for (i, session) in sessions.iter().enumerate() {
        let modified = format_time(session.modified);
        let project = session.project_path.chars().take(20).collect::<String>();

        println!(
            "{:<4} {:<20} {:<30} {}",
            i + 1,
            project,
            modified,
            session.message_count
        );

        if let Some(ref prompt) = session.last_prompt {
            println!("     └─ {}", prompt.chars().take(60).collect::<String>());
        }
    }

    println!();
    println!("Use 'claude-tools resume show <#>' to view session details");
    println!("Use 'claude-tools resume open <#>' to open project directory");

    Ok(())
}

fn show_session(session_ref: &str) -> Result<()> {
    let session = resolve_session(session_ref)?;

    println!("Session: {}", session.id);
    println!("Project: {}", session.project_path);
    println!("Path: {}", session.path.display());
    println!("Messages: {}", session.message_count);
    println!();

    // Show last few messages
    let content = fs::read_to_string(&session.path)?;
    let lines: Vec<&str> = content.lines().collect();

    println!("Recent messages:");
    println!("{}", "-".repeat(60));

    for line in lines.iter().rev().take(10).rev() {
        if let Ok(msg) = serde_json::from_str::<SessionMessage>(line) {
            if let Some(ref content) = msg.message {
                let role = content.role.as_deref().unwrap_or("?");
                let text = match &content.content {
                    Some(serde_json::Value::String(s)) => s.chars().take(100).collect::<String>(),
                    Some(v) => format!("{:?}", v).chars().take(100).collect(),
                    None => "(no content)".to_string(),
                };

                let role_display = match role {
                    "user" => "\x1b[32muser\x1b[0m",
                    "assistant" => "\x1b[34massistant\x1b[0m",
                    _ => role,
                };

                println!("[{}] {}", role_display, text);
            }
        }
    }

    Ok(())
}

fn open_session(session_ref: &str) -> Result<()> {
    let session = resolve_session(session_ref)?;

    // Try to find the actual project directory
    let project_dir = if PathBuf::from(&session.project_path).exists() {
        session.project_path.clone()
    } else {
        // Fall back to session file's parent directory
        session.path.parent()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|| ".".to_string())
    };

    println!("Opening: {}", project_dir);
    println!("Run 'claude' to start a new session in this project.");

    // Open a new terminal or print cd command
    println!();
    println!("cd \"{}\"", project_dir);

    Ok(())
}

fn resolve_session(session_ref: &str) -> Result<Session> {
    let sessions = find_sessions(100)?;

    // Try to parse as index first
    if let Ok(idx) = session_ref.parse::<usize>() {
        if idx > 0 && idx <= sessions.len() {
            return Ok(sessions[idx - 1].clone());
        }
    }

    // Try to match by ID
    for session in sessions {
        if session.id.contains(session_ref) {
            return Ok(session);
        }
    }

    anyhow::bail!("Session not found: {}", session_ref)
}

fn format_time(time: std::time::SystemTime) -> String {
    let duration = time.duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
    let secs = duration.as_secs();

    // Simple relative time formatting
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let diff = now.saturating_sub(secs);

    if diff < 60 {
        "just now".to_string()
    } else if diff < 3600 {
        format!("{} minutes ago", diff / 60)
    } else if diff < 86400 {
        format!("{} hours ago", diff / 3600)
    } else {
        format!("{} days ago", diff / 86400)
    }
}

impl Clone for Session {
    fn clone(&self) -> Self {
        Session {
            id: self.id.clone(),
            path: self.path.clone(),
            project_path: self.project_path.clone(),
            modified: self.modified,
            message_count: self.message_count,
            last_prompt: self.last_prompt.clone(),
        }
    }
}
