//! Long-running work with persisted, broadcast progress.

use std::sync::Arc;

use tokio::sync::Mutex;

use crate::error::AppResult;
use crate::events::EventBus;
use crate::model::{Job, JobKind, JobState};
use crate::store::Store;
use crate::util::{new_id, now_iso};

#[derive(Clone)]
pub struct Jobs {
    store: Store,
    events: EventBus,
}

impl Jobs {
    pub fn new(store: Store, events: EventBus) -> Self {
        Self { store, events }
    }

    pub async fn create(&self, kind: JobKind, target_id: &str, message: impl Into<String>) -> AppResult<JobHandle> {
        let job = Job {
            id: new_id("job"),
            kind,
            target_id: target_id.to_owned(),
            state: JobState::Running,
            progress: None,
            message: message.into(),
            error: None,
            created_at: now_iso(),
            finished_at: None,
        };
        self.store.put_job(&job).await?;
        self.events.job(&job);
        Ok(JobHandle { inner: Arc::new(Mutex::new(job)), store: self.store.clone(), events: self.events.clone() })
    }
}

/// A running job. Cloning shares the same job.
#[derive(Clone)]
pub struct JobHandle {
    inner: Arc<Mutex<Job>>,
    store: Store,
    events: EventBus,
}

impl JobHandle {
    pub async fn snapshot(&self) -> Job {
        self.inner.lock().await.clone()
    }

    pub async fn id(&self) -> String {
        self.inner.lock().await.id.clone()
    }

    /// Updates progress (0..1, or `None` when unknown) and the status line.
    pub async fn progress(&self, progress: Option<f64>, message: impl Into<String>) {
        let job = {
            let mut job = self.inner.lock().await;
            job.progress = progress.map(|value| value.clamp(0.0, 1.0));
            job.message = message.into();
            job.clone()
        };
        // Progress is broadcast every time but persisted best-effort.
        self.events.job(&job);
        let _ = self.store.put_job(&job).await;
    }

    pub async fn succeed(&self, message: impl Into<String>) {
        self.finish(JobState::Succeeded, message.into(), None).await;
    }

    pub async fn fail(&self, error: impl Into<String>) {
        let error = error.into();
        self.finish(JobState::Failed, error.clone(), Some(error)).await;
    }

    async fn finish(&self, state: JobState, message: String, error: Option<String>) {
        let job = {
            let mut job = self.inner.lock().await;
            job.state = state;
            job.message = message;
            job.error = error;
            if state == JobState::Succeeded {
                job.progress = Some(1.0);
            }
            job.finished_at = Some(now_iso());
            job.clone()
        };
        self.events.job(&job);
        if let Err(err) = self.store.put_job(&job).await {
            tracing::warn!(error = %err, "could not persist job");
        }
    }
}
