//! Project-wide constants.

pub const AUTHOR: &str = "Assaf Sapir";
pub const HOMEPAGE: &str = "sapir.io";
pub const REPO: &str = "github.com/assapir/golem";

/// Default Anthropic model when none is specified.
pub const DEFAULT_MODEL: &str = "claude-sonnet-4-20250514";

/// Format a number with comma separators (e.g. 1,234,567).
pub fn format_number(n: u64) -> String {
    let s = n.to_string();
    let mut result = String::with_capacity(s.len() + s.len() / 3);
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            result.push(',');
        }
        result.push(c);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn consts_are_non_empty() {
        assert!(!AUTHOR.is_empty());
        assert!(!HOMEPAGE.is_empty());
        assert!(!REPO.is_empty());
        assert!(!DEFAULT_MODEL.is_empty());
    }

    #[test]
    fn repo_contains_github() {
        assert!(REPO.contains("github.com"));
    }

    #[test]
    fn format_number_zero() {
        assert_eq!(format_number(0), "0");
    }

    #[test]
    fn format_number_small() {
        assert_eq!(format_number(42), "42");
        assert_eq!(format_number(999), "999");
    }

    #[test]
    fn format_number_thousands() {
        assert_eq!(format_number(1_000), "1,000");
        assert_eq!(format_number(1_234), "1,234");
        assert_eq!(format_number(12_345), "12,345");
        assert_eq!(format_number(123_456), "123,456");
    }

    #[test]
    fn format_number_millions() {
        assert_eq!(format_number(1_000_000), "1,000,000");
        assert_eq!(format_number(1_234_567), "1,234,567");
    }

    #[test]
    fn format_number_single_digit() {
        assert_eq!(format_number(1), "1");
    }
}
