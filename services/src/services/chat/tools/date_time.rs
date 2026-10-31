//! The `date_time` tool: the current date and time.

use chrono::SecondsFormat;
use views::conversations::ToolCallView;

use crate::chat::tools::ToolOutcome;
use crate::infrastructure::Clock;

/// The current time from `clock`, in UTC and labelled as such so the model knows.
pub fn run(clock: &dyn Clock) -> ToolOutcome {
    // Built by hand: `json!` objects sort their keys, and the model reads the time first.
    let utc = clock.now().to_rfc3339_opts(SecondsFormat::Secs, true);
    ToolOutcome {
        content: format!(r#"{{"utc":"{utc}","timezone":"UTC"}}"#),
        is_error: false,
        display: ToolCallView { name: "date_time".into(), query: None, media_type: None, count: None, titles: None, failed: false },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{fixed_clock, init};

    #[test]
    fn run_returns_current_utc_time_when_called() {
        init();

        let outcome = run(&fixed_clock(2024, 1, 1, 12, 0, 0));

        assert_eq!(outcome.content, r#"{"utc":"2024-01-01T12:00:00Z","timezone":"UTC"}"#);
        assert!(!outcome.is_error);
        assert_eq!(outcome.display.name, "date_time");
        assert!(!outcome.display.failed);
    }
}
