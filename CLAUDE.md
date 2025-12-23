# Claude Tools

CLI utilities that integrate with and complement Claude Code.

## Tools

### claude-monitor

TUI dashboard for monitoring multiple Claude Code sessions across tmux panes.

**Features:**
- Real-time status detection (waiting, working, permission required, idle)
- Auto-refresh with configurable interval
- Filter to show only Claude Code sessions or all panes
- Keyboard navigation

**Usage:**
```bash
claude-monitor              # Default: 2s refresh, Claude panes only
claude-monitor -a           # Show all panes
claude-monitor -i 5         # Refresh every 5 seconds
claude-monitor --help       # Full options
```

**Status indicators:**
- `>_` Green - Waiting for input
- `..` Yellow - Working/thinking
- `?!` Red - Permission required
- `ok` Gray - Idle/completed

## Project Structure

```
claude-tools/
├── src/
│   ├── main.rs       # CLI args, event loop, terminal setup
│   ├── app.rs        # Application state management
│   ├── detector.rs   # Claude Code status detection from pane content
│   ├── tmux.rs       # tmux interaction (list panes, capture content)
│   └── ui.rs         # TUI rendering with ratatui
├── Cargo.toml
└── CLAUDE.md
```

## Development

### Build & Run
```bash
cargo build --release
cargo run

# Install locally
cargo install --path .
```

### Testing
```bash
cargo test
cargo clippy
cargo fmt
```

### Code Style
- Use `anyhow` for error handling
- Use `clap` derive macros for CLI args
- Unit tests in same file with `#[cfg(test)]` modules
- Follow Rust idioms and rustfmt style

## Dependencies

- `ratatui` + `crossterm` - TUI framework
- `clap` - CLI argument parsing
- `anyhow` - Error handling
- `tokio` - Async runtime
- `serde` / `serde_json` - Serialization
