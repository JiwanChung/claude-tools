use anyhow::{Context, Result};
use std::process::Command;

/// Switch to a specific pane (works across sessions)
pub fn switch_to_pane(session: &str, window: u32, pane: u32) -> Result<()> {
    let target = format!("{}:{}.{}", session, window, pane);

    // First, switch the client to the target session (enables cross-session navigation)
    Command::new("tmux")
        .args(["switch-client", "-t", session])
        .output()
        .context("Failed to switch session")?;

    // Then select the window within that session
    Command::new("tmux")
        .args(["select-window", "-t", &format!("{}:{}", session, window)])
        .output()
        .context("Failed to select window")?;

    // Finally select the specific pane
    Command::new("tmux")
        .args(["select-pane", "-t", &target])
        .output()
        .context("Failed to select pane")?;

    Ok(())
}

/// Send keys to a specific pane
pub fn send_keys(pane_id: &str, keys: &str) -> Result<()> {
    Command::new("tmux")
        .args(["send-keys", "-t", pane_id, keys])
        .output()
        .context("Failed to send keys to pane")?;

    Ok(())
}

#[derive(Debug, Clone)]
pub struct Pane {
    pub id: String,
    pub session_name: String,
    pub window_index: u32,
    pub pane_index: u32,
    pub current_path: String,
    pub tty: String,
    pub title: String,
}

impl Pane {
    pub fn display_name(&self) -> String {
        format!(
            "{}:{}.{}",
            self.session_name, self.window_index, self.pane_index
        )
    }
}

pub fn list_panes() -> Result<Vec<Pane>> {
    let output = Command::new("tmux")
        .args([
            "list-panes",
            "-a",
            "-F",
            "#{pane_id}\t#{session_name}\t#{window_index}\t#{pane_index}\t#{pane_current_path}\t#{pane_tty}\t#{pane_title}",
        ])
        .output()
        .context("Failed to execute tmux list-panes")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        if stderr.contains("no server running") || stderr.contains("no current client") {
            return Ok(Vec::new());
        }
        anyhow::bail!("tmux list-panes failed: {}", stderr);
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let panes = stdout
        .lines()
        .filter_map(|line| {
            let parts: Vec<&str> = line.split('\t').collect();
            if parts.len() >= 7 {
                Some(Pane {
                    id: parts[0].to_string(),
                    session_name: parts[1].to_string(),
                    window_index: parts[2].parse().unwrap_or(0),
                    pane_index: parts[3].parse().unwrap_or(0),
                    current_path: parts[4].to_string(),
                    tty: parts[5].to_string(),
                    title: parts[6].to_string(),
                })
            } else {
                None
            }
        })
        .collect();

    Ok(panes)
}

pub fn capture_pane(pane_id: &str, lines: usize) -> Result<String> {
    let start_line = format!("-{}", lines);
    let output = Command::new("tmux")
        .args(["capture-pane", "-t", pane_id, "-p", "-S", &start_line])
        .output()
        .context("Failed to execute tmux capture-pane")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("tmux capture-pane failed: {}", stderr);
    }

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pane_display_name() {
        let pane = Pane {
            id: "%0".to_string(),
            session_name: "dev".to_string(),
            window_index: 1,
            pane_index: 0,
            current_path: "/home/user".to_string(),
            tty: "/dev/ttys001".to_string(),
            title: "test title".to_string(),
        };
        assert_eq!(pane.display_name(), "dev:1.0");
    }
}
