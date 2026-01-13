use crate::detector::{self, DetectionResult, Status};
use crate::tmux::{self, Pane};
use std::time::Duration;
use anyhow::Result;
use std::collections::{HashMap, HashSet};
use std::time::Instant;
use serde::Serialize;

#[derive(Debug, Clone)]
pub struct PaneState {
    pub pane: Pane,
    pub status: DetectionResult,
    pub last_content_hash: u64,
    pub last_change: Instant,
    pub status_changed_at: Instant,
    pub previous_status: Option<Status>,
    pub content_preview: String,
    // Stats tracking
    pub stats: PaneStats,
}

#[derive(Debug, Clone, Default)]
pub struct PaneStats {
    pub total_working_secs: u64,
    pub total_waiting_secs: u64,
    pub total_permission_secs: u64,
    pub state_changes: u32,
}

impl PaneState {
    pub fn status_duration(&self) -> Duration {
        self.status_changed_at.elapsed()
    }

    pub fn status_duration_str(&self) -> String {
        format_duration(self.status_duration())
    }
}

pub fn format_duration(d: Duration) -> String {
    let secs = d.as_secs();
    if secs < 60 {
        format!("{}s", secs)
    } else if secs < 3600 {
        format!("{}m{}s", secs / 60, secs % 60)
    } else {
        format!("{}h{}m", secs / 3600, (secs % 3600) / 60)
    }
}

pub struct App {
    pub pane_states: HashMap<String, PaneState>,
    pub selected_index: usize,
    pub capture_lines: usize,
    pub show_all_panes: bool,
    pub compact_mode: bool,
    pub group_by_session: bool,
    pub show_stats: bool,
    pub collapsed_sessions: HashSet<String>,
    pub status_filter: Option<Status>,
    pub self_pane_id: Option<String>,
}

#[derive(Debug, Clone)]
pub struct StateChangeNotification {
    pub pane_name: String,
    pub folder_name: String,
    pub session_name: String,
    pub window_index: u32,
    pub pane_index: u32,
    pub is_permission: bool,
}

impl App {
    pub fn new(capture_lines: usize, show_all: bool, compact: bool) -> Self {
        // Get our own pane ID to exclude from monitoring
        let self_pane_id = std::env::var("TMUX_PANE").ok();

        Self {
            pane_states: HashMap::new(),
            selected_index: 0,
            capture_lines,
            show_all_panes: show_all,
            compact_mode: compact,
            group_by_session: false,
            show_stats: false,
            collapsed_sessions: HashSet::new(),
            status_filter: None,
            self_pane_id,
        }
    }

    pub fn refresh(&mut self) -> Result<Vec<StateChangeNotification>> {
        let panes = tmux::list_panes()?;

        // Track which panes we've seen
        let mut seen_ids: Vec<String> = Vec::new();
        let mut notifications: Vec<StateChangeNotification> = Vec::new();

        for pane in panes {
            // Skip our own pane
            if Some(&pane.id) == self.self_pane_id.as_ref() {
                continue;
            }

            seen_ids.push(pane.id.clone());

            let content = tmux::capture_pane(&pane.id, self.capture_lines).unwrap_or_default();
            let content_hash = hash_content(&content);
            let status = detector::detect_status_from_session(&pane.tty, &content, Some(&pane.title));

            // Get preview (last non-empty line)
            let preview = content
                .lines()
                .rev()
                .find(|l| !l.trim().is_empty())
                .unwrap_or("")
                .chars()
                .take(60)
                .collect::<String>();

            // Extract folder name for notifications
            let folder_name = pane
                .current_path
                .rsplit('/')
                .next()
                .unwrap_or(&pane.current_path)
                .to_string();

            if let Some(existing) = self.pane_states.get_mut(&pane.id) {
                // Update existing state
                if existing.last_content_hash != content_hash {
                    existing.last_change = Instant::now();
                    existing.last_content_hash = content_hash;
                }
                // Track status changes
                if existing.status.status != status.status {
                    // Accumulate time in previous state
                    let elapsed_secs = existing.status_changed_at.elapsed().as_secs();
                    match existing.status.status {
                        Status::Working => existing.stats.total_working_secs += elapsed_secs,
                        Status::WaitingForInput => existing.stats.total_waiting_secs += elapsed_secs,
                        Status::PermissionRequired => existing.stats.total_permission_secs += elapsed_secs,
                        Status::NotClaudeCode => {}
                    }
                    existing.stats.state_changes += 1;

                    // Generate notification for Working -> WaitingForInput or PermissionRequired
                    if existing.status.status == Status::Working
                        && (status.status == Status::WaitingForInput
                            || status.status == Status::PermissionRequired)
                    {
                        notifications.push(StateChangeNotification {
                            pane_name: pane.display_name(),
                            folder_name: folder_name.clone(),
                            session_name: pane.session_name.clone(),
                            window_index: pane.window_index,
                            pane_index: pane.pane_index,
                            is_permission: status.status == Status::PermissionRequired,
                        });
                    }
                    existing.previous_status = Some(existing.status.status);
                    existing.status_changed_at = Instant::now();
                }
                existing.pane = pane;
                existing.status = status;
                existing.content_preview = preview;
            } else {
                // New pane
                self.pane_states.insert(
                    pane.id.clone(),
                    PaneState {
                        pane,
                        status,
                        last_content_hash: content_hash,
                        last_change: Instant::now(),
                        status_changed_at: Instant::now(),
                        previous_status: None,
                        content_preview: preview,
                        stats: PaneStats::default(),
                    },
                );
            }
        }

        // Remove panes that no longer exist
        self.pane_states.retain(|id, _| seen_ids.contains(id));

        // Adjust selected index if needed
        let visible_count = self.visible_panes().len();
        if visible_count > 0 && self.selected_index >= visible_count {
            self.selected_index = visible_count - 1;
        }

        Ok(notifications)
    }

    pub fn visible_panes(&self) -> Vec<&PaneState> {
        let mut panes: Vec<&PaneState> = self
            .pane_states
            .values()
            .filter(|p| self.show_all_panes || p.status.status != Status::NotClaudeCode)
            .filter(|p| {
                match self.status_filter {
                    Some(filter) => p.status.status == filter,
                    None => true,
                }
            })
            .collect();

        // Sort by status (Permission first, then Working), then by session/window/pane
        panes.sort_by(|a, b| {
            let status_order = |s: Status| match s {
                Status::PermissionRequired => 0,
                Status::Working => 1,
                Status::WaitingForInput => 2,
                Status::NotClaudeCode => 3,
            };

            status_order(a.status.status)
                .cmp(&status_order(b.status.status))
                .then(a.pane.session_name.cmp(&b.pane.session_name))
                .then(a.pane.window_index.cmp(&b.pane.window_index))
                .then(a.pane.pane_index.cmp(&b.pane.pane_index))
        });

        panes
    }

    pub fn select_next(&mut self) {
        let count = self.visible_panes().len();
        if count > 0 {
            self.selected_index = (self.selected_index + 1) % count;
        }
    }

    pub fn select_previous(&mut self) {
        let count = self.visible_panes().len();
        if count > 0 {
            if self.selected_index == 0 {
                self.selected_index = count - 1;
            } else {
                self.selected_index -= 1;
            }
        }
    }

    pub fn toggle_show_all(&mut self) {
        self.show_all_panes = !self.show_all_panes;
        // Reset selection when toggling
        self.selected_index = 0;
    }

    pub fn toggle_compact(&mut self) {
        self.compact_mode = !self.compact_mode;
    }

    pub fn toggle_filter_working(&mut self) {
        self.status_filter = match self.status_filter {
            Some(Status::Working) => None,
            _ => Some(Status::Working),
        };
        self.selected_index = 0;
    }

    pub fn toggle_filter_waiting(&mut self) {
        self.status_filter = match self.status_filter {
            Some(Status::WaitingForInput) => None,
            _ => Some(Status::WaitingForInput),
        };
        self.selected_index = 0;
    }

    pub fn toggle_grouping(&mut self) {
        self.group_by_session = !self.group_by_session;
    }

    pub fn toggle_stats(&mut self) {
        self.show_stats = !self.show_stats;
    }

    /// Get aggregated stats across all visible panes
    pub fn aggregated_stats(&self) -> AggregatedStats {
        let panes = self.visible_panes();
        let mut stats = AggregatedStats::default();

        for pane in &panes {
            // Add accumulated stats
            stats.total_working_secs += pane.stats.total_working_secs;
            stats.total_waiting_secs += pane.stats.total_waiting_secs;
            stats.total_permission_secs += pane.stats.total_permission_secs;
            stats.total_state_changes += pane.stats.state_changes;

            // Add current state time
            let current_secs = pane.status_changed_at.elapsed().as_secs();
            match pane.status.status {
                Status::Working => stats.total_working_secs += current_secs,
                Status::WaitingForInput => stats.total_waiting_secs += current_secs,
                Status::PermissionRequired => stats.total_permission_secs += current_secs,
                Status::NotClaudeCode => {}
            }
        }

        stats.pane_count = panes.len();
        stats
    }

    pub fn toggle_session_collapse(&mut self, session: &str) {
        if self.collapsed_sessions.contains(session) {
            self.collapsed_sessions.remove(session);
        } else {
            self.collapsed_sessions.insert(session.to_string());
        }
    }

    pub fn selected_pane(&self) -> Option<&PaneState> {
        let panes = self.visible_panes();
        panes.get(self.selected_index).copied()
    }

    pub fn summary(&self) -> StatusSummary {
        let panes = self.visible_panes();
        StatusSummary {
            total: panes.len(),
            waiting: panes
                .iter()
                .filter(|p| p.status.status == Status::WaitingForInput)
                .count(),
            permission: panes
                .iter()
                .filter(|p| p.status.status == Status::PermissionRequired)
                .count(),
            working: panes
                .iter()
                .filter(|p| p.status.status == Status::Working)
                .count(),
        }
    }
}

#[derive(Debug)]
pub struct StatusSummary {
    pub total: usize,
    pub waiting: usize,
    pub permission: usize,
    pub working: usize,
}

#[derive(Debug, Default)]
pub struct AggregatedStats {
    pub pane_count: usize,
    pub total_working_secs: u64,
    pub total_waiting_secs: u64,
    pub total_permission_secs: u64,
    pub total_state_changes: u32,
}

impl AggregatedStats {
    pub fn efficiency_percent(&self) -> f64 {
        let total = self.total_working_secs + self.total_waiting_secs + self.total_permission_secs;
        if total == 0 {
            0.0
        } else {
            (self.total_working_secs as f64 / total as f64) * 100.0
        }
    }
}

fn hash_content(content: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    content.hash(&mut hasher);
    hasher.finish()
}

#[derive(Serialize)]
pub struct ExportData {
    pub timestamp: String,
    pub summary: ExportSummary,
    pub panes: Vec<ExportPane>,
}

#[derive(Serialize)]
pub struct ExportSummary {
    pub total_panes: usize,
    pub total_working_secs: u64,
    pub total_waiting_secs: u64,
    pub total_permission_secs: u64,
    pub total_state_changes: u32,
    pub efficiency_percent: f64,
}

#[derive(Serialize)]
pub struct ExportPane {
    pub session: String,
    pub window: u32,
    pub pane: u32,
    pub path: String,
    pub current_status: String,
    pub working_secs: u64,
    pub waiting_secs: u64,
    pub permission_secs: u64,
    pub state_changes: u32,
}

impl App {
    pub fn export_stats(&self) -> ExportData {
        let stats = self.aggregated_stats();
        let panes = self.visible_panes();

        let export_panes: Vec<ExportPane> = panes
            .iter()
            .map(|p| {
                let current_secs = p.status_changed_at.elapsed().as_secs();
                let mut working = p.stats.total_working_secs;
                let mut waiting = p.stats.total_waiting_secs;
                let mut permission = p.stats.total_permission_secs;

                // Add current state time
                match p.status.status {
                    Status::Working => working += current_secs,
                    Status::WaitingForInput => waiting += current_secs,
                    Status::PermissionRequired => permission += current_secs,
                    Status::NotClaudeCode => {}
                }

                ExportPane {
                    session: p.pane.session_name.clone(),
                    window: p.pane.window_index,
                    pane: p.pane.pane_index,
                    path: p.pane.current_path.clone(),
                    current_status: p.status.status.label().to_string(),
                    working_secs: working,
                    waiting_secs: waiting,
                    permission_secs: permission,
                    state_changes: p.stats.state_changes,
                }
            })
            .collect();

        ExportData {
            timestamp: chrono_lite_now(),
            summary: ExportSummary {
                total_panes: stats.pane_count,
                total_working_secs: stats.total_working_secs,
                total_waiting_secs: stats.total_waiting_secs,
                total_permission_secs: stats.total_permission_secs,
                total_state_changes: stats.total_state_changes,
                efficiency_percent: stats.efficiency_percent(),
            },
            panes: export_panes,
        }
    }
}

fn chrono_lite_now() -> String {
    use std::time::SystemTime;
    let duration = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default();
    format!("{}", duration.as_secs())
}
