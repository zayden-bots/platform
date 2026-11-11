use std::collections::HashMap;
use std::time::Duration;

use futures::future;
use jiff::tz::TimeZone;
use jiff::{SignedDuration, Timestamp};
use tokio::task::JoinHandle;
use tokio::time::{sleep, timeout};
use tracing::{Instrument, debug, error, info_span, warn};
use zayden_core::{
    CronCtx,
    PendingJob,
    WorkerJob,
    earliest_pending,
    prune_exhausted,
};

use crate::Result;

const DRAIN: Duration = Duration::from_secs(8);

pub(super) async fn run(
    mut jobs: Vec<WorkerJob>,
    cx: CronCtx,
    shutdown: impl Future<Output = ()>,
) -> Result<()> {
    tokio::pin!(shutdown);
    let mut running: HashMap<String, JoinHandle<()>> = HashMap::new();

    loop {
        let now = Timestamp::now().to_zoned(TimeZone::UTC);
        prune_exhausted(&mut jobs, &now);
        let pending_jobs: Vec<PendingJob<WorkerJob>> = earliest_pending(&jobs, &now);

        let sleep_duration = pending_jobs.first().map_or_else(
            || SignedDuration::new(60, 0),
            |next| {
                debug!("Next Job: {:?}", next.at);

                let now = Timestamp::now().to_zoned(TimeZone::UTC);
                if next.at > now {
                    next.at.duration_since(&now)
                } else {
                    SignedDuration::new(0, 0)
                }
            },
        );

        if sleep_duration > SignedDuration::new(1, 0) {
            let std_duration = sleep_duration.try_into()?;
            tokio::select! {
                () = sleep(std_duration) => {},
                () = &mut shutdown => break,
            }
        }

        if !pending_jobs.is_empty() {
            running.retain(|_, handle| !handle.is_finished());

            for job in pending_jobs {
                if running.contains_key(&job.id) {
                    warn!(job = job.id, "previous run still going; skipped");
                    continue;
                }

                let id = job.id.clone();
                let action = (job.action)(cx.clone());
                let span = info_span!("cron_job", job = job.id);
                let job_id = id.clone();
                let handle = tokio::spawn(async move {
                    if let Err(e) = tokio::spawn(action.instrument(span)).await {
                        error!(job = job_id, error = ?e, "worker job panicked");
                    }
                });
                running.insert(id, handle);
            }
        }

        tokio::select! {
            () = sleep(Duration::from_secs(5)) => {},
            () = &mut shutdown => break,
        }
    }

    let remaining = running.len();
    if timeout(DRAIN, future::join_all(running.into_values())).await.is_err() {
        warn!(running = remaining, "worker jobs still running at shutdown");
    }
    Ok(())
}
