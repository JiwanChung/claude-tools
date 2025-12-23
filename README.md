# claude-tools

**A powerful CLI toolkit for managing Claude Code sessions, workflows, and resources.**

[![Rust](https://img.shields.io/badge/rust-1.70%2B-orange.svg)](https://www.rust-lang.org/)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![PRs Welcome](https://img.shields.io/badge/PRs-welcome-brightgreen.svg)](CONTRIBUTING.md)

---

## Overview

`claude-tools` is a companion CLI for [Claude Code](https://claude.ai/claude-code) that helps you:

- **Monitor** multiple Claude Code sessions across tmux panes in real-time
- **Resume** previous sessions with full context
- **Sync** your `CLAUDE.md` guidelines across projects
- **Track** token usage and set budget limits

```
┌─────────────────────────────────────────────────────────────────┐
│ Claude Code Monitor                              q:quit r:refresh│
├─────────────────────────────────────────────────────────────────┤
│ ▶ Session: dev (3 panes)                                        │
│   ├─ myapp          ⏳ Waiting    12.5k tokens   5m ago         │
│   ├─ api-server     🔄 Working    8.2k tokens    Edit: user.rs  │
│   └─ docs           ⚠️  Permission               Bash: rm -rf   │
├─────────────────────────────────────────────────────────────────┤
│ ▶ Session: work (2 panes)                                       │
│   ├─ backend        ⏳ Waiting    45.1k tokens   2h ago         │
│   └─ frontend       🔄 Working    22.3k tokens   Read: App.tsx  │
└─────────────────────────────────────────────────────────────────┘
```

## Installation

### Using Cargo

```bash
cargo install --git https://github.com/JiwanChung/claude-tools
```

### From source

```bash
git clone https://github.com/JiwanChung/claude-tools.git
cd claude-tools
cargo install --path .
```

### Requirements

- Rust 1.70+
- tmux (for monitor feature)
- macOS or Linux

## Commands

### `monitor` — Real-time Session Dashboard

Monitor all Claude Code sessions running in tmux panes with a beautiful TUI.

```bash
claude-tools monitor
```

**Features:**
- Live status detection: Waiting, Working, Permission Required
- Session grouping with collapsible sections
- Token count and timing statistics
- Desktop notifications on state changes
- Auto-jump to panes needing attention
- Quick approval with `y` key

**Options:**
```
-i, --interval <SECS>   Refresh interval [default: 2]
-l, --lines <NUM>       Lines to capture per pane [default: 100]
-a, --all               Show all panes, not just Claude Code
-c, --compact           Compact single-line view
-n, --notify            Enable desktop notifications
-j, --jump              Auto-jump to ready panes
```

**Keybindings:**
| Key | Action |
|-----|--------|
| `q` / `Esc` | Quit |
| `r` | Refresh |
| `j` / `↓` | Select next |
| `k` / `↑` | Select previous |
| `Enter` | Jump to pane |
| `y` | Approve permission |
| `g` | Toggle session grouping |
| `Tab` | Collapse/expand session |
| `s` | Toggle stats view |
| `e` | Export stats to JSON |
| `w` | Filter: working only |
| `i` | Filter: waiting only |
| `a` | Show all panes |
| `c` | Toggle compact mode |

---

### `resume` — Session History

List, view, and restore previous Claude Code sessions.

```bash
# List recent sessions
claude-tools resume list

# Show session details
claude-tools resume show 1

# Get project directory
claude-tools resume open 3
```

**Output:**
```
#    Project              Last Modified                  Messages
----------------------------------------------------------------------
1    myapp                5 minutes ago                  142
     └─ Add dark mode toggle to settings page
2    api-server           2 hours ago                    89
     └─ Fix authentication middleware
3    docs                 1 days ago                     34
     └─ Update API documentation
```

---

### `sync` — CLAUDE.md Management

Keep your `CLAUDE.md` project guidelines synchronized across repositories.

```bash
# Initialize a master template
claude-tools sync init

# Push to all projects (prepends by default)
claude-tools sync push ~/projects/*

# Check sync status
claude-tools sync status ~/projects/*

# View differences
claude-tools sync diff ~/.claude/CLAUDE.md ~/myapp/CLAUDE.md
```

**Strategies:**
| Strategy | Description |
|----------|-------------|
| `prepend` | Add global guidelines before project-specific content (default) |
| `append` | Add global guidelines after project-specific content |
| `replace` | Overwrite with global guidelines |

```bash
# Explicit strategy
claude-tools sync push -m replace ~/projects/*

# Dry run to preview changes
claude-tools sync push --dry-run ~/projects/*
```

---

### `budget` — Token Usage Tracking

Monitor and limit your Claude Code token consumption.

```bash
# View current usage
claude-tools budget status

# Set limits
claude-tools budget set --daily 500k --weekly 2m --monthly 10m

# Detailed report
claude-tools budget report --days 30 --group-by project
```

**Output:**
```
Token Usage Status
==================================================

Current Usage (last 30 days):
  Input tokens:  701.5k
  Output tokens: 2.6M
  Total:         3.3M
  Sessions:      136

Budget Limits:
  Daily:   450.2k/500k (90%) OK
  Weekly:  1.8M/2M (90%) OK
  Monthly: 3.3M/10M (33%) OK
```

**Limit formats:** `100k`, `1.5m`, `1000000`

---

## Configuration

Configuration is stored in `~/.claude/`:

```
~/.claude/
├── CLAUDE.md          # Master project guidelines
├── budget.json        # Budget limits
└── projects/          # Session history (auto-generated by Claude Code)
    └── <project>/
        └── <session>.jsonl
```

## How It Works

`claude-tools` is entirely local and doesn't make any API calls:

- **Monitor**: Reads tmux pane content via `tmux capture-pane`
- **Resume**: Parses JSONL session logs from `~/.claude/projects/`
- **Sync**: Manages `CLAUDE.md` files across directories
- **Budget**: Aggregates token usage from session logs

## Contributing

Contributions are welcome! Please feel free to submit a Pull Request.

1. Fork the repository
2. Create your feature branch (`git checkout -b feature/amazing-feature`)
3. Commit your changes (`git commit -m 'Add amazing feature'`)
4. Push to the branch (`git push origin feature/amazing-feature`)
5. Open a Pull Request

## License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.

## Acknowledgments

Built for use with [Claude Code](https://claude.ai/claude-code) by Anthropic.

---

<p align="center">
  <sub>Made with Rust and Claude</sub>
</p>
