//! Shared debug mode flag for printing raw LLM request/response data.
//!
//! `DebugMode` wraps an `Arc<AtomicBool>` so it can be cloned across
//! the engine, thinkers, and REPL commands while sharing a single flag.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// Shared, cloneable debug toggle. Default is off.
#[derive(Debug, Clone)]
pub struct DebugMode(Arc<AtomicBool>);

impl DebugMode {
    pub fn new(enabled: bool) -> Self {
        Self(Arc::new(AtomicBool::new(enabled)))
    }

    pub fn is_enabled(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }

    pub fn enable(&self) {
        self.0.store(true, Ordering::Relaxed);
    }

    pub fn disable(&self) {
        self.0.store(false, Ordering::Relaxed);
    }

    /// Toggle debug mode, returning the new state.
    pub fn toggle(&self) -> bool {
        // fetch_xor with true flips the bit
        let old = self.0.fetch_xor(true, Ordering::Relaxed);
        !old
    }

    /// Print a debug message to stderr if debug mode is enabled.
    pub fn log(&self, msg: &str) {
        if self.is_enabled() {
            eprintln!("[debug] {msg}");
        }
    }
}

impl Default for DebugMode {
    fn default() -> Self {
        Self::new(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_off() {
        let debug = DebugMode::default();
        assert!(!debug.is_enabled());
    }

    #[test]
    fn new_respects_initial_state() {
        assert!(DebugMode::new(true).is_enabled());
        assert!(!DebugMode::new(false).is_enabled());
    }

    #[test]
    fn enable_and_disable() {
        let debug = DebugMode::default();
        debug.enable();
        assert!(debug.is_enabled());
        debug.disable();
        assert!(!debug.is_enabled());
    }

    #[test]
    fn toggle_flips_state() {
        let debug = DebugMode::default();
        assert!(debug.toggle()); // off → on
        assert!(debug.is_enabled());
        assert!(!debug.toggle()); // on → off
        assert!(!debug.is_enabled());
    }

    #[test]
    fn clone_shares_state() {
        let a = DebugMode::default();
        let b = a.clone();
        a.enable();
        assert!(b.is_enabled());
        b.disable();
        assert!(!a.is_enabled());
    }

    #[test]
    fn toggle_returns_new_state() {
        let debug = DebugMode::new(false);
        let new = debug.toggle();
        assert!(new);
        assert!(debug.is_enabled());

        let new = debug.toggle();
        assert!(!new);
        assert!(!debug.is_enabled());
    }
}
