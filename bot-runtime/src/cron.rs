use std::sync::Arc;
use std::time::Duration;

use futures::future;
use jiff::tz::TimeZone;
use jiff::{SignedDuration, Timestamp};
use serenity::all::Context;
use sqlx::PgPool;
use tokio::sync::RwLock;
use tokio::time::sleep;
use tracing::{Instrument, debug, error, info, info_span};
use zayden_app::entitlement::EntitlementService;
use zayden_app::tasks::{PLATFORM_STALE_AFTER, ScheduledTasks};
use zayden_core::{
    CronJob,
    CronResult,
    DM_TASK,
    POST_TASK,
    PendingJob,
    RuntimeState,
    WorkerJob,
    WorkerJobResult,
    earliest_pending,
    prune_exhausted,
};

use crate::Result;

pub struct EntitlementSweepCron;

impl EntitlementSweepCron {
    pub fn worker_job(entitlements: Arc<EntitlementService>) -> WorkerJobResult {
        WorkerJob::new("entitlement_expiry_sweep", "0 0 * * * * *").map(|job| {
            job.set_action(move |_cx| {
                let entitlements = Arc::clone(&entitlements);
                async move {
                    match entitlements.refresh_expired_cache_rows().await {
                        Ok(n) if n > 0 => {
                            info!(demoted = n, "entitlement expiry sweep completed");
                        },
                        Ok(_) => {},
                        Err(e) => {
                            error!(error = ?e, "entitlement expiry sweep failed");
                        },
                    }
                }
            })
        })
    }
}

pub struct PlatformTaskGc;

impl PlatformTaskGc {
    pub fn worker_job(db: PgPool) -> WorkerJobResult {
        WorkerJob::new("platform_task_gc", "0 30 * * * * *").map(|job| {
            job.set_action(move |_cx| {
                let db = db.clone();
                async move {
                    match ScheduledTasks::purge_unclaimed(
                        &db,
                        &[POST_TASK, DM_TASK],
                        PLATFORM_STALE_AFTER,
                    )
                    .await
                    {
                        Ok(n) if n > 0 => {
                            info!(purged = n, "platform task gc completed");
                        },
                        Ok(_) => {},
                        Err(e) => {
                            error!(error = ?e, "platform task gc failed");
                        },
                    }
                }
            })
        })
    }
}

#[must_use]
pub fn collect_jobs(jobs: Vec<CronResult>) -> Vec<CronJob> {
    jobs.into_iter()
        .filter_map(|job| match job {
            Ok(job) => Some(job),
            Err(e) => {
                error!(error = ?e, "failed to create cron job");
                None
            },
        })
        .collect()
}

pub async fn start_cron_jobs(ctx: Context, pool: PgPool) {
    if let Err(e) = run_cron_jobs_loop(ctx, pool).await {
        error!("Error starting cron jobs: {e:?}");
    }
}

async fn run_cron_jobs_loop(ctx: Context, pool: PgPool) -> Result<()> {
    loop {
        let pending_jobs = pending_jobs(&ctx).await;

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
            let futures_iter = pending_jobs.into_iter().map(|job| {
                (job.action)(ctx.clone(), pool.clone())
                    .instrument(info_span!("cron_job", job = job.id))
            });

            future::join_all(futures_iter).await;
        }

        sleep(Duration::from_secs(5)).await;
    }
}

async fn pending_jobs(ctx: &Context) -> Vec<PendingJob> {
    let data = ctx.data::<RwLock<RuntimeState>>();

    let now = Timestamp::now().to_zoned(TimeZone::UTC);

    let mut state = data.write().await;

    prune_exhausted(state.jobs_mut(), &now);
    earliest_pending(state.jobs(), &now)
}
