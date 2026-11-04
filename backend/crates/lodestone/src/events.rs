//! The global event bus behind `GET /events`.

use tokio::sync::broadcast;

use crate::model::{Job, LodestoneEvent, ResourceKind, ServerState};

#[derive(Clone)]
pub struct EventBus {
    tx: broadcast::Sender<LodestoneEvent>,
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new()
    }
}

impl EventBus {
    pub fn new() -> Self {
        let (tx, _) = broadcast::channel(512);
        Self { tx }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<LodestoneEvent> {
        self.tx.subscribe()
    }

    pub fn publish(&self, event: LodestoneEvent) {
        // No subscribers is fine: nobody has the UI open.
        let _ = self.tx.send(event);
    }

    pub fn changed(&self, resource: ResourceKind, id: impl Into<Option<String>>) {
        self.publish(LodestoneEvent::Changed { resource, id: id.into() });
    }

    pub fn deleted(&self, resource: ResourceKind, id: &str) {
        self.publish(LodestoneEvent::Deleted { resource, id: id.to_owned() });
    }

    pub fn instance_state(&self, id: &str, state: ServerState) {
        self.publish(LodestoneEvent::InstanceState { id: id.to_owned(), state });
    }

    pub fn job(&self, job: &Job) {
        self.publish(LodestoneEvent::Job { job: job.clone() });
    }
}
