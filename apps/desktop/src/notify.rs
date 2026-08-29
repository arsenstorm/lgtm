//! OS notifications. GPUI has none of its own and both platforms ship a
//! command that does the job, so this shells out rather than take a crate.

use std::process::Command;

/// Best effort: no `notify-send` on PATH, or a user who refused notifications,
/// must not disturb the app.
pub fn send(title: &str, body: &str) {
    if let Some(mut command) = command(title, body) {
        let _ = command.spawn();
    }
}

#[cfg(target_os = "macos")]
fn command(title: &str, body: &str) -> Option<Command> {
    let mut command = Command::new("osascript");
    command.arg("-e").arg(format!(
        "display notification \"{}\" with title \"{}\"",
        escape(body),
        escape(title)
    ));
    Some(command)
}

/// An AppleScript string literal. Backslashes go first, or this would escape
/// the backslashes it just added for the quotes.
#[cfg(target_os = "macos")]
fn escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

#[cfg(target_os = "linux")]
fn command(title: &str, body: &str) -> Option<Command> {
    let mut command = Command::new("notify-send");
    command.arg(title).arg(body);
    Some(command)
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn command(_title: &str, _body: &str) -> Option<Command> {
    None
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;

    #[test]
    fn escaping_closes_no_applescript_string() {
        assert_eq!(escape(r#"say "hi" \ bye"#), r#"say \"hi\" \\ bye"#);
    }
}
