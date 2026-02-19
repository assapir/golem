//! Decoupled event bus for cross-component communication.
//!
//! Components emit events via [`EventBus::emit`] and subscribe via
//! [`EventBus::subscribe`]. Built on [`tokio::sync::broadcast`] so
//! multiple listeners can react independently.

use tokio::sync::broadcast;

/// Events that flow through the system.
#[derive(Debug, Clone)]
pub enum Event {
    /// The active model was changed (carries the new model ID).
    ModelChanged { model: String },
}

/// A broadcast channel that any component can emit to or subscribe from.
#[derive(Debug)]
pub struct EventBus {
    tx: broadcast::Sender<Event>,
}

impl EventBus {
    /// Create a new event bus with the given channel capacity.
    pub fn new(capacity: usize) -> Self {
        let (tx, _) = broadcast::channel(capacity);
        Self { tx }
    }

    /// Emit an event to all current subscribers.
    /// Returns the number of receivers that will see it.
    pub fn emit(&self, event: Event) -> usize {
        self.tx.send(event).unwrap_or(0)
    }

    /// Subscribe to events. Returns a receiver that yields all
    /// future events (does not replay past ones).
    pub fn subscribe(&self) -> broadcast::Receiver<Event> {
        self.tx.subscribe()
    }
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new(16)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn emit_reaches_subscriber() {
        let bus = EventBus::default();
        let mut rx = bus.subscribe();

        bus.emit(Event::ModelChanged {
            model: "claude-sonnet-4-20250514".to_string(),
        });

        let event = rx.recv().await.unwrap();
        match event {
            Event::ModelChanged { model } => assert_eq!(model, "claude-sonnet-4-20250514"),
        }
    }

    #[tokio::test]
    async fn multiple_subscribers_receive_event() {
        let bus = EventBus::default();
        let mut rx1 = bus.subscribe();
        let mut rx2 = bus.subscribe();

        bus.emit(Event::ModelChanged {
            model: "opus".to_string(),
        });

        let e1 = rx1.recv().await.unwrap();
        let e2 = rx2.recv().await.unwrap();

        match (e1, e2) {
            (Event::ModelChanged { model: m1 }, Event::ModelChanged { model: m2 }) => {
                assert_eq!(m1, "opus");
                assert_eq!(m2, "opus");
            }
        }
    }

    #[test]
    fn emit_without_subscribers_returns_zero() {
        let bus = EventBus::default();
        let count = bus.emit(Event::ModelChanged {
            model: "test".to_string(),
        });
        assert_eq!(count, 0);
    }

    #[test]
    fn emit_with_subscriber_returns_count() {
        let bus = EventBus::default();
        let _rx1 = bus.subscribe();
        let _rx2 = bus.subscribe();

        let count = bus.emit(Event::ModelChanged {
            model: "test".to_string(),
        });
        assert_eq!(count, 2);
    }
}
