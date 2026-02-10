//! Reusable TUI components for user prompts.

mod config_view;
mod message;
mod multiselect;
mod progress;
mod select;
mod text_input;

pub use config_view::{ConfigEntry, ConfigView};
pub use message::{Message, MessageStyle};
pub use multiselect::{MultiSelect, MultiSelectItem};
pub use progress::{Progress, ProgressHandle};
pub use select::{Select, SelectItem};
pub use text_input::TextInput;

use anyhow::Result;

/// Result of input validation.
#[derive(Debug, Clone)]
pub enum ValidationResult {
    /// Input is valid
    Valid,
    /// Input is invalid with an error message
    Invalid(String),
}

impl ValidationResult {
    /// Returns true if the validation passed.
    #[must_use]
    pub fn is_valid(&self) -> bool {
        matches!(self, Self::Valid)
    }
}

/// Result type for prompt operations.
///
/// - `Ok(Some(value))` - User submitted a value
/// - `Ok(None)` - User cancelled (Escape or Ctrl+C)
/// - `Err(_)` - An error occurred
pub type PromptResult<T> = Result<Option<T>>;

/// Estimates how many terminal rows `text` will occupy when wrapped to `width` columns.
///
/// Parameters:
/// - `text`: the string to measure
/// - `width`: available column width
///
/// Returns: number of rows (minimum 1)
#[allow(clippy::arithmetic_side_effects, clippy::cast_possible_truncation)]
pub(crate) fn wrapped_line_count(text: &str, width: u16) -> u16 {
    if width == 0 {
        return 1;
    }
    let w = width as usize;
    // Count characters (not bytes) for a closer approximation of display width
    let len = text.chars().count();
    len.div_ceil(w).max(1) as u16
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wrapped_line_count_empty_string() {
        // Empty string still occupies at least 1 row
        assert_eq!(wrapped_line_count("", 80), 1);
    }

    #[test]
    fn test_wrapped_line_count_zero_width() {
        // Zero width is a degenerate case, should return 1 to avoid division by zero
        assert_eq!(wrapped_line_count("hello", 0), 1);
    }

    #[test]
    fn test_wrapped_line_count_fits_single_line() {
        assert_eq!(wrapped_line_count("short", 80), 1);
    }

    #[test]
    fn test_wrapped_line_count_exact_fit() {
        // 10 chars in width 10 = exactly 1 line
        assert_eq!(wrapped_line_count("0123456789", 10), 1);
    }

    #[test]
    fn test_wrapped_line_count_wraps_to_two_lines() {
        // 11 chars in width 10 = 2 lines
        assert_eq!(wrapped_line_count("01234567890", 10), 2);
    }

    #[test]
    fn test_wrapped_line_count_wraps_to_three_lines() {
        // 25 chars in width 10 = 3 lines
        assert_eq!(wrapped_line_count("0123456789012345678901234", 10), 3);
    }

    #[test]
    fn test_wrapped_line_count_width_one() {
        // Each character gets its own line
        assert_eq!(wrapped_line_count("abc", 1), 3);
    }

    #[test]
    fn test_wrapped_line_count_unicode_chars() {
        // Unicode chars: "héllo" is 5 chars, should fit in width 10
        assert_eq!(wrapped_line_count("héllo", 10), 1);
        // 11 unicode chars in width 5 = 3 lines
        assert_eq!(wrapped_line_count("àbcdéfghïjk", 5), 3);
    }

    #[test]
    fn test_wrapped_line_count_long_help_message() {
        // Simulate the actual uninstall confirmation help text in a narrow terminal
        let text = "This will permanently remove all hooks, configuration, data, and the binary. This cannot be undone.";
        // In a 40-col dialog inner width, ~99 chars = 3 lines
        assert_eq!(wrapped_line_count(text, 40), 3);
    }
}
