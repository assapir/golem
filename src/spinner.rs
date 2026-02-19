//! A minimal terminal spinner for visual feedback during async operations.

use std::io::Write;
use std::time::Duration;

use tokio::task::JoinHandle;

/// Braille spinner frames.
const FRAMES: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

/// Frame interval.
const INTERVAL: Duration = Duration::from_millis(80);

/// A terminal spinner that runs in a background task.
///
/// Call [`Spinner::start`] to begin, then [`Spinner::stop`] when done.
/// The spinner writes to stderr so it doesn't interfere with stdout output.
pub struct Spinner {
    handle: JoinHandle<()>,
    cancel: tokio::sync::watch::Sender<bool>,
}

impl Spinner {
    /// Start a spinner with the given message (e.g. `"thinking"`).
    pub fn start(message: &str) -> Self {
        let (cancel_tx, mut cancel_rx) = tokio::sync::watch::channel(false);
        let message = message.to_string();

        let handle = tokio::spawn(async move {
            let mut i = 0;
            loop {
                let frame = FRAMES[i % FRAMES.len()];
                // \r moves to start of line, \x1b[2K clears the line
                eprint!("\x1b[2K\r{frame} {message}");
                let _ = std::io::stderr().flush();

                tokio::select! {
                    _ = tokio::time::sleep(INTERVAL) => {}
                    _ = cancel_rx.changed() => break,
                }
                i += 1;
            }
            // Clear the spinner line
            eprint!("\x1b[2K\r");
            let _ = std::io::stderr().flush();
        });

        Self {
            handle,
            cancel: cancel_tx,
        }
    }

    /// Stop the spinner and clear its line.
    pub async fn stop(self) {
        let _ = self.cancel.send(true);
        let _ = self.handle.await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_are_non_empty() {
        assert!(!FRAMES.is_empty());
        for frame in FRAMES {
            assert!(!frame.is_empty());
        }
    }

    #[test]
    fn frames_are_single_braille_chars() {
        for frame in FRAMES {
            assert_eq!(frame.chars().count(), 1);
        }
    }

    #[tokio::test]
    async fn spinner_starts_and_stops_without_panic() {
        let spinner = Spinner::start("testing");
        tokio::time::sleep(Duration::from_millis(200)).await;
        spinner.stop().await;
    }

    #[tokio::test]
    async fn spinner_immediate_stop() {
        let spinner = Spinner::start("quick");
        spinner.stop().await;
    }
}
