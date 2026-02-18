//! Startup banner and session summary display.

use std::path::Path;

use crate::consts::{AUTHOR, HOMEPAGE, REPO, format_number};
use crate::thinker::TokenUsage;

/// Session configuration for display in the startup banner.
pub struct BannerInfo<'a> {
    pub provider: &'a str,
    pub model: &'a str,
    pub auth_status: &'a str,
    pub shell_mode: &'a str,
    pub working_dir: &'a Path,
    pub memory: &'a str,
}

/// Print the startup banner with session info.
pub fn print_banner(info: &BannerInfo) {
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
   auth      {}
   shell     {}
   workdir   {}
   memory    {}
"#,
        env!("CARGO_PKG_VERSION"),
        AUTHOR,
        HOMEPAGE,
        REPO,
        info.provider,
        info.model,
        info.auth_status,
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
        let info = BannerInfo {
            provider: "human",
            model: "—",
            auth_status: "N/A",
            shell_mode: "read-only",
            working_dir: &PathBuf::from("/tmp/test"),
            memory: "ephemeral",
        };
        // Just verify it doesn't panic
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
