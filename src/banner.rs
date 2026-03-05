//! Startup banner and session summary display.

use std::path::Path;

use crate::consts::{AUTHOR, HOMEPAGE, REPO, format_number};
use crate::provider::ProviderStatus;
use crate::thinker::TokenUsage;

/// Auth status for a single provider in the banner.
pub struct BannerProvider<'a> {
    pub display_name: &'a str,
    pub auth_status: &'a str,
    pub is_active: bool,
}

/// Session configuration for display in the startup banner.
pub struct BannerInfo<'a> {
    pub provider: &'a str,
    pub model: &'a str,
    pub providers: &'a [ProviderStatus],
    pub shell_mode: &'a str,
    pub working_dir: &'a Path,
    pub memory: &'a str,
}

/// Print the startup banner with session info.
pub fn print_banner(info: &BannerInfo) {
    // Build auth lines aligned with other banner fields.
    // Field label column is 10 chars ("provider  ", "shell     ", etc.).
    let mut auth_lines = String::new();
    for status in info.providers {
        let marker = if status.id == info.provider {
            " ← active"
        } else {
            ""
        };
        let label = if auth_lines.is_empty() {
            "auth      "
        } else {
            "          "
        };
        auth_lines.push_str(&format!(
            "   {label}{} ({}){}\n",
            status.display_name, status.auth_status, marker,
        ));
    }

    // Fallback if no providers
    if auth_lines.is_empty() {
        auth_lines = "   auth      N/A\n".to_string();
    }

    println!(
        r#"
   ╔═══════════════════════════════════════╗
   ║              G O L E M                ║
   ║     a clay body, animated by words    ║
   ╚═══════════════════════════════════════╝

   version   {}
   by        {}
   home      {}
   repo      {}
   provider  {} ({})
{}   shell     {}
   workdir   {}
   memory    {}
"#,
        env!("CARGO_PKG_VERSION"),
        AUTHOR,
        HOMEPAGE,
        REPO,
        info.provider,
        info.model,
        auth_lines,
        info.shell_mode,
        info.working_dir.display(),
        info.memory,
    );
}

/// Print the session summary (token usage + farewell).
pub fn print_session_summary(usage: TokenUsage) {
    if usage.total() > 0 {
        println!(
            "session: {:>6} input + {:>6} output = {:>6} tokens",
            format_number(usage.input_tokens),
            format_number(usage.output_tokens),
            format_number(usage.total()),
        );
    }
    println!("goodbye.");
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn print_banner_does_not_panic() {
        let statuses = vec![ProviderStatus {
            id: "anthropic",
            display_name: "Anthropic (Claude Pro/Max)",
            auth_status: "OAuth ✓".to_string(),
        }];
        let info = BannerInfo {
            provider: "anthropic",
            model: "claude-sonnet-4-20250514",
            providers: &statuses,
            shell_mode: "read-only",
            working_dir: &PathBuf::from("/tmp/test"),
            memory: "ephemeral",
        };
        // Just verify it doesn't panic
        print_banner(&info);
    }

    #[test]
    fn print_banner_multiple_providers() {
        let statuses = vec![
            ProviderStatus {
                id: "anthropic",
                display_name: "Anthropic (Claude Pro/Max)",
                auth_status: "OAuth ✓".to_string(),
            },
            ProviderStatus {
                id: "google",
                display_name: "Google (Gemini)",
                auth_status: "not authenticated".to_string(),
            },
        ];
        let info = BannerInfo {
            provider: "anthropic",
            model: "claude-sonnet-4-20250514",
            providers: &statuses,
            shell_mode: "read-only",
            working_dir: &PathBuf::from("/tmp/test"),
            memory: "ephemeral",
        };
        print_banner(&info);
    }

    #[test]
    fn print_banner_no_providers() {
        let info = BannerInfo {
            provider: "human",
            model: "—",
            providers: &[],
            shell_mode: "read-only",
            working_dir: &PathBuf::from("/tmp/test"),
            memory: "ephemeral",
        };
        print_banner(&info);
    }

    #[test]
    fn print_session_summary_with_tokens() {
        let usage = TokenUsage {
            input_tokens: 1234,
            output_tokens: 567,
        };
        // Just verify it doesn't panic
        print_session_summary(usage);
    }

    #[test]
    fn print_session_summary_zero_tokens() {
        // Should only print "goodbye." with no token line
        print_session_summary(TokenUsage::default());
    }
}
