use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    /// Waiting for user input
    WaitingForInput,
    /// Waiting for permission approval
    PermissionRequired,
    /// Actively working (thinking, tool execution)
    Working,
    /// Not a Claude Code session
    NotClaudeCode,
}

impl Status {
    pub fn icon(&self) -> &'static str {
        match self {
            Status::WaitingForInput => ">_",
            Status::PermissionRequired => "⚠",
            Status::Working => "◐",
            Status::NotClaudeCode => "--",
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Status::WaitingForInput => "Waiting for input",
            Status::PermissionRequired => "Permission required",
            Status::Working => "Working",
            Status::NotClaudeCode => "Not Claude Code",
        }
    }
}

impl fmt::Display for Status {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}", self.icon(), self.label())
    }
}

#[derive(Debug, Clone)]
pub struct DetectionResult {
    pub status: Status,
    pub detail: Option<String>,
    pub tokens: Option<String>,
}

/// Detect Claude Code status from pane content
pub fn detect_status(content: &str) -> DetectionResult {
    // Check if this looks like Claude Code at all
    if !is_claude_code_session(content) {
        return DetectionResult {
            status: Status::NotClaudeCode,
            detail: None,
            tokens: None,
        };
    }

    // Simple sentinel: "esc to interrupt" means Claude is working
    if content.contains("esc to interrupt") {
        let tokens = extract_tokens(content);
        let last_command = extract_last_user_command(content);
        return DetectionResult {
            status: Status::Working,
            detail: last_command,
            tokens,
        };
    }

    // Check for permission prompts
    if is_permission_prompt(content) {
        let permission_detail = extract_permission_detail(content);
        return DetectionResult {
            status: Status::PermissionRequired,
            detail: permission_detail,
            tokens: None,
        };
    }

    // Otherwise, Claude is waiting for input
    // Extract last action (line starting with ⏺)
    let last_action = extract_last_action(content);

    DetectionResult {
        status: Status::WaitingForInput,
        detail: last_action,
        tokens: None,
    }
}

/// Check if content shows a permission prompt
fn is_permission_prompt(content: &str) -> bool {
    // Look for permission-related patterns in the last portion of content
    let last_lines: String = content.lines().rev().take(20).collect::<Vec<_>>().join("\n");

    // Claude Code permission patterns
    let patterns = [
        "Allow",           // Permission button
        "Deny",            // Permission button
        "Yes, allow",      // Yes option
        "allow this",      // Allow this action
        "Yes, proceed",    // Yes proceed
        "allow once",      // Allow once
        "allow always",    // Allow always
    ];

    for pattern in &patterns {
        if last_lines.contains(pattern) {
            return true;
        }
    }

    false
}

/// Extract what permission is being requested
fn extract_permission_detail(content: &str) -> Option<String> {
    // Find the tool/action that's requesting permission
    // Usually appears after ⏺ marker
    for line in content.lines().rev() {
        let trimmed = line.trim();
        if trimmed.starts_with("⏺") {
            let action = trimmed.trim_start_matches("⏺").trim();
            if !action.is_empty() {
                return Some(action.chars().take(60).collect());
            }
        }
    }
    Some("Tool permission".to_string())
}

/// Extract token count from thinking indicator (e.g., "↓ 7.0k tokens")
fn extract_tokens(content: &str) -> Option<String> {
    // Look for pattern like "↓ 7.0k tokens" or "↓ 1.2k tokens"
    for line in content.lines().rev() {
        if line.contains("tokens") && line.contains("↓") {
            // Find the token count pattern
            if let Some(pos) = line.find("↓") {
                let after = &line[pos..];
                // Skip the ↓ character (3 bytes in UTF-8) and any space
                let arrow_len = "↓".len();
                if after.len() > arrow_len {
                    let rest = &after[arrow_len..];
                    let rest = rest.trim_start();
                    // Extract until "tokens"
                    if let Some(end) = rest.find("tokens") {
                        let token_part = rest[..end].trim();
                        return Some(format!("{}tokens", token_part));
                    }
                }
            }
        }
    }
    None
}

/// Extract the last user command (text after > prompt)
fn extract_last_user_command(content: &str) -> Option<String> {
    let lines: Vec<&str> = content.lines().collect();

    // Find lines that start with > (user prompt)
    // The user's command follows immediately after
    let mut command_lines: Vec<&str> = Vec::new();
    let mut in_command = false;

    for line in &lines {
        let trimmed = line.trim();

        // Skip empty lines and separator lines
        if trimmed.is_empty() || trimmed.chars().all(|c| c == '─') {
            if in_command && !command_lines.is_empty() {
                // End of command block
                break;
            }
            continue;
        }

        // Check if this is a prompt line
        if trimmed == ">" || trimmed.starts_with("> ") {
            // Reset - new command starting
            command_lines.clear();
            in_command = true;

            // If there's text after >, capture it
            if trimmed.len() > 2 {
                command_lines.push(&trimmed[2..]);
            }
            continue;
        }

        // If we're in a command and hit a Claude response marker, stop
        if in_command && (trimmed.starts_with("⏺") || trimmed.starts_with("✢")) {
            break;
        }

        // If we're in a command, collect the line
        if in_command {
            command_lines.push(trimmed);
        }
    }

    if command_lines.is_empty() {
        return None;
    }

    // Join and truncate
    let result = command_lines.join(" ");
    let truncated: String = result.chars().take(80).collect();

    if truncated.len() < result.len() {
        Some(format!("{}...", truncated))
    } else {
        Some(truncated)
    }
}

/// Extract everything after the last ⏺ marker
fn extract_last_action(content: &str) -> Option<String> {
    // Find the last occurrence of ⏺
    let last_marker_pos = content.rfind("⏺")?;

    // Get everything from that marker onwards
    let after_marker = &content[last_marker_pos..];

    // Clean up: remove the ⏺ itself, trim, and limit lines
    let cleaned: Vec<&str> = after_marker
        .lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty())
        .filter(|l| !l.chars().all(|c| c == '─')) // Skip separator lines
        .filter(|l| !l.starts_with('>')) // Skip prompt lines
        .filter(|l| !l.contains("bypass permissions")) // Skip permission indicator
        .take(5) // Limit to 5 lines
        .collect();

    if cleaned.is_empty() {
        return None;
    }

    // Join with newlines, removing the ⏺ prefix from first line
    let mut result = cleaned.join("\n");
    if result.starts_with("⏺") {
        result = result.trim_start_matches("⏺").trim().to_string();
    }

    Some(result)
}

fn is_claude_code_session(content: &str) -> bool {
    // Claude Code specific UI elements
    let indicators = [
        "⏺ ",       // Tool call marker
        "⎿",        // Tool output marker
        "✢",        // Thinking indicator
        "⏵⏵",      // Permission mode indicator
        "Claude Code",
    ];

    for indicator in &indicators {
        if content.contains(indicator) {
            return true;
        }
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_not_claude_code() {
        let content = "$ ls -la\ntotal 0\ndrwxr-xr-x  2 user  staff  64 Dec 23 10:00 .";
        let result = detect_status(content);
        assert_eq!(result.status, Status::NotClaudeCode);
    }

    #[test]
    fn test_detect_working() {
        let content = "⏺ Read(file.txt)\n✢ Mulling… (esc to interrupt · 1m 30s)";
        let result = detect_status(content);
        assert_eq!(result.status, Status::Working);
    }

    #[test]
    fn test_detect_waiting() {
        let content = "⏺ Done with the task.\n─────────────────────────────────────\n> \n─────────────────────────────────────";
        let result = detect_status(content);
        assert_eq!(result.status, Status::WaitingForInput);
    }

    #[test]
    fn test_claude_code_detection() {
        let content = "⏺ Bash(ls -la)\n  ⎿  total 0";
        assert!(is_claude_code_session(content));
    }
}
