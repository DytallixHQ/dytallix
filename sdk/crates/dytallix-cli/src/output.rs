//! Shared terminal output formatting for the Dytallix CLI.

use std::time::Duration;

use colored::Colorize;

/// Prints a success line, optionally including an elapsed duration.
pub fn success(message: &str, elapsed: Option<Duration>) {
    println!("{}", format_success(message, elapsed));
}

/// Prints an error line in red.
pub fn error(message: &str) {
    println!("{}", format_error(message).red());
}

/// Prints the standard Dytallix divider line.
pub fn divider() {
    println!("{}", divider_string());
}

/// Prints a section header wrapped in divider lines.
pub fn section(title: &str) {
    divider();
    println!("  {title}");
    divider();
}

fn divider_string() -> &'static str {
    "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
}

fn format_success(message: &str, elapsed: Option<Duration>) -> String {
    match elapsed {
        Some(duration) => format!("✓ {message}    [{:.1}s]", duration.as_secs_f64()),
        None => format!("✓ {message}"),
    }
}

fn format_error(message: &str) -> String {
    format!("✗ {message}")
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::{divider_string, format_error, format_success};

    #[test]
    fn formatters_match_expected_strings() {
        assert_eq!(format_success("ready", None), "✓ ready");
        assert_eq!(format_error("failed"), "✗ failed");
        assert_eq!(divider_string(), "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
        assert!(format_success("ready", Some(Duration::from_millis(150))).contains("[0.1s]"));
    }
}
