use crate::pricing::{self, SessionCost};
use anyhow::{Context, Result};
use serde::Deserialize;
use std::fs;
use std::io::{BufRead, BufReader, Seek, SeekFrom};
use std::path::PathBuf;
use std::process::Command;

/// Information about a Claude Code session derived from its files
#[derive(Debug, Clone, Default)]
pub struct SessionInfo {
    pub session_id: Option<String>,
    pub cwd: Option<String>,
    pub last_user_prompt: Option<String>,
    pub current_tool: Option<String>,
    pub tool_detail: Option<String>,
    pub model: Option<String>,
    pub stop_reason: Option<String>,
    pub is_streaming: bool,
    pub is_idle: bool,
    pub session_cost: Option<SessionCost>,
}

/// Find Claude process PID by TTY
pub fn find_claude_pid_by_tty(tty: &str) -> Option<u32> {
    // Strip /dev/ prefix if present for ps matching
    let tty_name = tty.trim_start_matches("/dev/");

    let output = Command::new("ps")
        .args(["-t", tty_name, "-o", "pid=,comm="])
        .output()
        .ok()?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    for line in stdout.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() >= 2 && parts[1] == "claude" {
            return parts[0].parse().ok();
        }
    }
    None
}

/// Get the current working directory of a process
pub fn get_process_cwd(pid: u32) -> Option<String> {
    let output = Command::new("lsof")
        .args(["-p", &pid.to_string()])
        .output()
        .ok()?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    for line in stdout.lines() {
        if line.contains("cwd") {
            // lsof output format: COMMAND PID USER FD TYPE DEVICE SIZE/OFF NODE NAME
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 9 {
                return Some(parts[8..].join(" "));
            }
        }
    }
    None
}

/// Encode a path to Claude's project folder format
/// Claude Code replaces /, _, ., and space with -
fn encode_path(path: &str) -> String {
    path.chars()
        .map(|c| match c {
            '/' | '_' | '.' | ' ' => '-',
            _ => c,
        })
        .collect()
}

/// Get the Claude projects directory
fn claude_projects_dir() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(".claude").join("projects"))
}

/// Get the Claude debug directory
fn claude_debug_dir() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(".claude").join("debug"))
}

/// Get the Claude history file
fn claude_history_file() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(".claude").join("history.jsonl"))
}

/// Find the most recent session JSONL file for a project
pub fn find_session_jsonl(cwd: &str) -> Option<PathBuf> {
    let projects_dir = claude_projects_dir()?;
    let encoded = encode_path(cwd);
    let project_dir = projects_dir.join(&encoded);

    if !project_dir.exists() {
        return None;
    }

    // Find most recently modified .jsonl file (not in subdirs, not agent- prefixed)
    let mut jsonl_files: Vec<_> = fs::read_dir(&project_dir)
        .ok()?
        .filter_map(|e| e.ok())
        .filter(|e| {
            let name = e.file_name();
            let name_str = name.to_string_lossy();
            name_str.ends_with(".jsonl") && !name_str.starts_with("agent-")
        })
        .collect();

    jsonl_files.sort_by_key(|e| {
        e.metadata()
            .and_then(|m| m.modified())
            .unwrap_or(std::time::SystemTime::UNIX_EPOCH)
    });

    jsonl_files.last().map(|e| e.path())
}

/// Get the debug file for a session
pub fn get_debug_file(session_id: &str) -> Option<PathBuf> {
    let debug_dir = claude_debug_dir()?;
    let debug_file = debug_dir.join(format!("{}.txt", session_id));
    if debug_file.exists() {
        Some(debug_file)
    } else {
        None
    }
}

#[derive(Deserialize)]
struct JsonlEntry {
    #[serde(rename = "sessionId")]
    session_id: Option<String>,
    message: Option<JsonlMessage>,
}

#[derive(Deserialize)]
struct HistoryEntry {
    #[serde(rename = "sessionId")]
    session_id: Option<String>,
    display: Option<String>,
}

#[derive(Deserialize)]
struct JsonlMessage {
    role: Option<String>,
    model: Option<String>,
    stop_reason: Option<String>,
    content: Option<serde_json::Value>,
}

/// Get the last user prompt from history.jsonl for a session
pub fn get_last_prompt_from_history(session_id: &str) -> Option<String> {
    let history_file = claude_history_file()?;
    let file = fs::File::open(&history_file).ok()?;
    let file_size = file.metadata().ok()?.len();

    // Read last ~50KB to find recent prompts
    let mut reader = BufReader::new(file);
    let start_pos = file_size.saturating_sub(50_000);
    reader.seek(SeekFrom::Start(start_pos)).ok()?;

    if start_pos > 0 {
        let mut skip = String::new();
        reader.read_line(&mut skip).ok()?;
    }

    let mut last_prompt = None;
    for line in reader.lines().flatten() {
        if line.is_empty() {
            continue;
        }
        if let Ok(entry) = serde_json::from_str::<HistoryEntry>(&line) {
            if entry.session_id.as_deref() == Some(session_id) {
                if let Some(display) = entry.display {
                    last_prompt = Some(display);
                }
            }
        }
    }

    last_prompt
}

/// Parse context from the last few entries of a session JSONL
pub fn parse_session_context(jsonl_path: &PathBuf) -> Result<SessionInfo> {
    let file = fs::File::open(jsonl_path).context("Failed to open JSONL file")?;
    let file_size = file.metadata()?.len();

    // Read last ~100KB of the file to get recent entries
    let mut reader = BufReader::new(file);
    let start_pos = file_size.saturating_sub(100_000);
    reader.seek(SeekFrom::Start(start_pos))?;

    // If we seeked into the middle, skip the partial first line
    if start_pos > 0 {
        let mut skip = String::new();
        reader.read_line(&mut skip)?;
    }

    let mut info = SessionInfo::default();
    let mut entries: Vec<JsonlEntry> = Vec::new();

    for line in reader.lines().flatten() {
        if line.is_empty() {
            continue;
        }
        if let Ok(entry) = serde_json::from_str::<JsonlEntry>(&line) {
            entries.push(entry);
        }
    }

    // Process entries in reverse to find most recent info
    for entry in entries.iter().rev() {
        if info.session_id.is_none() {
            info.session_id = entry.session_id.clone();
        }

        if let Some(msg) = &entry.message {
            if info.model.is_none() {
                info.model = msg.model.clone();
            }
            if info.stop_reason.is_none() {
                info.stop_reason = msg.stop_reason.clone();
            }

            if let Some(content) = &msg.content {
                if let Some(arr) = content.as_array() {
                    for item in arr {
                        let item_type = item.get("type").and_then(|t| t.as_str());

                        // Get last user prompt (actual text, not tool results)
                        if info.last_user_prompt.is_none()
                            && item_type == Some("text")
                            && msg.role.as_deref() == Some("user")
                        {
                            if let Some(text) = item.get("text").and_then(|t| t.as_str()) {
                                // Skip system messages
                                if !text.starts_with('<') {
                                    info.last_user_prompt =
                                        Some(text.chars().take(150).collect());
                                }
                            }
                        }

                        // Get current/last tool
                        if info.current_tool.is_none() && item_type == Some("tool_use") {
                            if let Some(name) = item.get("name").and_then(|n| n.as_str()) {
                                info.current_tool = Some(name.to_string());

                                if let Some(input) = item.get("input") {
                                    info.tool_detail = match name {
                                        "Bash" => input
                                            .get("command")
                                            .and_then(|c| c.as_str())
                                            .map(|c| {
                                                c.lines().next().unwrap_or("").chars().take(60).collect()
                                            }),
                                        "Read" | "Edit" | "Write" => input
                                            .get("file_path")
                                            .and_then(|p| p.as_str())
                                            .and_then(|p| p.rsplit('/').next())
                                            .map(String::from),
                                        "Grep" => input
                                            .get("pattern")
                                            .and_then(|p| p.as_str())
                                            .map(|p| p.chars().take(40).collect()),
                                        "Task" => input
                                            .get("description")
                                            .and_then(|d| d.as_str())
                                            .map(|d| d.chars().take(40).collect()),
                                        _ => None,
                                    };
                                }
                            }
                        }
                    }
                }
            }
        }

        // Stop once we have all the info we need
        if info.session_id.is_some()
            && info.last_user_prompt.is_some()
            && info.current_tool.is_some()
        {
            break;
        }
    }

    Ok(info)
}

/// Parse the debug log to determine if Claude is actively streaming or idle
pub fn parse_debug_state(debug_path: &PathBuf) -> Result<(bool, bool)> {
    let file = fs::File::open(debug_path).context("Failed to open debug file")?;
    let file_size = file.metadata()?.len();

    // Read last ~10KB to find recent events
    let mut reader = BufReader::new(file);
    let start_pos = file_size.saturating_sub(10_000);
    reader.seek(SeekFrom::Start(start_pos))?;

    if start_pos > 0 {
        let mut skip = String::new();
        reader.read_line(&mut skip)?;
    }

    let mut last_stream_time: Option<&str> = None;
    let mut last_idle_time: Option<&str> = None;
    let lines: Vec<String> = reader.lines().filter_map(|l| l.ok()).collect();

    for line in lines.iter().rev().take(50) {
        if last_stream_time.is_none() && line.contains("Stream started") {
            // Extract timestamp (first 24 chars typically)
            if line.len() >= 24 {
                last_stream_time = Some(&line[..24]);
            }
        }
        if last_idle_time.is_none() && line.contains("idle_prompt") {
            if line.len() >= 24 {
                last_idle_time = Some(&line[..24]);
            }
        }
        if last_stream_time.is_some() && last_idle_time.is_some() {
            break;
        }
    }

    // Determine state based on which event is more recent
    let is_streaming = match (last_stream_time, last_idle_time) {
        (Some(stream), Some(idle)) => stream > idle,
        (Some(_), None) => true,
        _ => false,
    };

    let is_idle = match (last_stream_time, last_idle_time) {
        (Some(stream), Some(idle)) => idle > stream,
        (None, Some(_)) => true,
        _ => false,
    };

    Ok((is_streaming, is_idle))
}

/// Get full session info for a pane by TTY
pub fn get_session_info_by_tty(tty: &str) -> Option<SessionInfo> {
    let pid = find_claude_pid_by_tty(tty)?;
    let cwd = get_process_cwd(pid)?;
    let jsonl_path = find_session_jsonl(&cwd)?;

    let mut info = parse_session_context(&jsonl_path).ok()?;
    info.cwd = Some(cwd);

    // Calculate session cost from JSONL
    info.session_cost = pricing::calculate_session_cost(&jsonl_path);

    // Get streaming/idle state from debug log, and last prompt from history
    if let Some(session_id) = &info.session_id {
        if let Some(debug_path) = get_debug_file(session_id) {
            if let Ok((is_streaming, is_idle)) = parse_debug_state(&debug_path) {
                info.is_streaming = is_streaming;
                info.is_idle = is_idle;
            }
        }

        // Get last user prompt from history.jsonl
        if info.last_user_prompt.is_none() {
            info.last_user_prompt = get_last_prompt_from_history(session_id);
        }
    }

    Some(info)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encode_path() {
        assert_eq!(
            encode_path("/Users/jiwan/projects/test"),
            "-Users-jiwan-projects-test"
        );
        // Underscores become dashes
        assert_eq!(
            encode_path("/Users/jiwan/projects/cua_project"),
            "-Users-jiwan-projects-cua-project"
        );
        // Dots become dashes
        assert_eq!(
            encode_path("/Users/jiwan/.config/tmux"),
            "-Users-jiwan--config-tmux"
        );
    }
}
