use std::sync::Arc;
use std::time::Duration;
use tracing::{error, info};

use crate::handlers::JobContext;
use crate::queue::JobQueue;

pub struct JobWorker {
    ctx: Arc<JobContext>,
}

impl JobWorker {
    pub fn new(ctx: Arc<JobContext>) -> Self {
        Self { ctx }
    }

    pub async fn run(self) {
        info!("job worker started");
        // Re-queue interrupted running jobs on startup
        match self.ctx.queue.recover_interrupted().await {
            Ok(n) if n > 0 => info!(count = n, "re-queued interrupted jobs"),
            Ok(_) => {}
            Err(e) => error!(error = %e, "failed to recover interrupted jobs"),
        }

        loop {
            match self.tick().await {
                Ok(true) => {} // processed a job, immediately try next
                Ok(false) => {
                    tokio::time::sleep(Duration::from_millis(500)).await;
                }
                Err(e) => {
                    error!(error = %e, "worker tick error");
                    tokio::time::sleep(Duration::from_secs(2)).await;
                }
            }
        }
    }

    async fn tick(&self) -> Result<bool, pgpanel_core::Error> {
        let Some(op) = self.ctx.queue.claim_next().await? else {
            return Ok(false);
        };

        info!(
            operation_id = %op.id,
            job_type = op.job_type.as_str(),
            "running job"
        );

        match self.ctx.handle(op.clone()).await {
            Ok(result) => {
                if let Err(e) = self.ctx.queue.succeed(op.id, result).await {
                    error!(error = %e, "failed to mark job succeeded");
                }
            }
            Err(e) => {
                error!(operation_id = %op.id, error = %e, "job failed");
                if let Err(e2) = self.ctx.queue.fail(op.id, &e.to_string()).await {
                    error!(error = %e2, "failed to mark job failed");
                }
            }
        }
        Ok(true)
    }
}

/// Convenience to share queue access from API.
impl JobWorker {
    pub fn queue(&self) -> &JobQueue {
        &self.ctx.queue
    }
}
