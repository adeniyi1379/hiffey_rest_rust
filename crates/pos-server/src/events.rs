use pos_types::ServerEvent;
use tokio::sync::broadcast;

/// Fan-out channel for events pushed to every connected terminal.
#[derive(Clone)]
pub struct Events {
    tx: broadcast::Sender<ServerEvent>,
}

impl Events {
    #[must_use]
    pub fn new() -> Self {
        let (tx, _) = broadcast::channel(256);
        Self { tx }
    }

    #[must_use]
    pub fn subscribe(&self) -> broadcast::Receiver<ServerEvent> {
        self.tx.subscribe()
    }

    /// Sends an event. Fails silently when no terminal is connected.
    pub fn send(&self, event: impl Into<String>, data: serde_json::Value) {
        let _ = self.tx.send(ServerEvent {
            event: event.into(),
            data,
        });
    }
}

impl Default for Events {
    fn default() -> Self {
        Self::new()
    }
}
