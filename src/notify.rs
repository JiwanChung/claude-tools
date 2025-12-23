use std::process::Command;

/// Send a macOS desktop notification
pub fn send_notification(title: &str, message: &str) {
    let script = format!(
        r#"display notification "{}" with title "{}""#,
        escape_applescript(message),
        escape_applescript(title)
    );

    let _ = Command::new("osascript")
        .args(["-e", &script])
        .output();
}

fn escape_applescript(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}
