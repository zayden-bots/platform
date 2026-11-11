use std::time::Duration;

use futures::future;
use jiff::tz::TimeZone;
use jiff::{SignedDuration, Timestamp};
use tokio::time::sleep;
use tracing::{Instrument, debug, error, info_span};
use zayden_core::{
    CronCtx,
    PendingJob,
    WorkerJob,
    earliest_pending,
    prune_exhausted,
};

use crate::Result;

pub(super) async fn run(mut jobs: Vec<WorkerJob>, cx: CronCtx) -> Result<()> {
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
            sleep(std_duration).await;
        }

        if !pending_jobs.is_empty() {
            let handles = pending_jobs.into_iter().map(|job| {
                let id = job.id.clone();
                let handle = tokio::spawn(
                    (job.action)(cx.clone())
                        .instrument(info_span!("cron_job", job = job.id)),
                );
                (id, handle)
            });

            future::join_all(handles.map(|(id, handle)| async move {
                if let Err(e) = handle.await {
                    error!(job = id, error = ?e, "worker job panicked");
                }
            }))
            .await;
        }

        sleep(Duration::from_secs(5)).await;
    }
}
