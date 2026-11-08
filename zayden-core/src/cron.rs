use std::cmp::Ordering;
use std::fmt::Debug;
use std::pin::Pin;
use std::str::FromStr;
use std::sync::Arc;

use jiff::Zoned;
use jiff_cron::Schedule;
use serenity::all::Context;
use sqlx::PgPool;

pub type ActionFn = Arc<
    dyn Fn(Context, PgPool) -> Pin<Box<dyn Future<Output = ()> + Send>>
        + Send
        + Sync,
>;

pub type CronResult = Result<CronJob, jiff_cron::error::Error>;

pub struct PendingJob {
    pub at: Zoned,
    pub id: String,
    pub action: ActionFn,
}

#[derive(Clone)]
pub struct CronJob {
    pub id: String,
    pub schedule: Schedule,
    pub action_fn: ActionFn,
}

impl CronJob {
    pub fn new(
        id: impl Into<String>,
        source: &str,
    ) -> Result<Self, jiff_cron::error::Error> {
        Ok(Self {
            id: id.into(),
            schedule: Schedule::from_str(source)?,
            action_fn: Self::action_fn(|_, _| async {}),
        })
    }

    fn action_fn<F, Fut>(f: F) -> ActionFn
    where
        F: Fn(Context, PgPool) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = ()> + Send + 'static,
    {
        let action_closure = move |ctx, pool| {
            let future = f(ctx, pool);
            let boxed_dyn_future: Box<dyn Future<Output = ()> + Send> =
                Box::new(future);

            let pinned_future: Pin<Box<dyn Future<Output = ()> + Send>> =
                Box::into_pin(boxed_dyn_future);

            pinned_future
        };

        Arc::new(action_closure)
    }

    #[must_use]
    pub fn set_action<F, Fut>(mut self, f: F) -> Self
    where
        F: Fn(Context, PgPool) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = ()> + Send + 'static,
    {
        self.action_fn = Self::action_fn(f);
        self
    }
}

fn next_run(job: &CronJob, now: &Zoned) -> Option<Zoned> {
    job.schedule.after(now.clone()).find(|t| job.schedule.includes(t.clone()))
}

pub fn prune_exhausted(jobs: &mut Vec<CronJob>, now: &Zoned) {
    jobs.retain(|job| job.schedule.after(now.clone()).next().is_some());
}

#[must_use]
pub fn earliest_pending(jobs: &[CronJob], now: &Zoned) -> Vec<PendingJob> {
    let mut pending: Vec<PendingJob> = Vec::new();

    for job in jobs {
        let Some(run_time) = next_run(job, now) else {
            continue;
        };

        let entry = PendingJob {
            at: run_time,
            id: job.id.clone(),
            action: Arc::clone(&job.action_fn),
        };

        match pending.first().map(|p| entry.at.cmp(&p.at)) {
            Some(Ordering::Less) | None => {
                pending = vec![entry];
            },
            Some(Ordering::Equal) => {
                pending.push(entry);
            },
            Some(Ordering::Greater) => {},
        }
    }

    pending
}

impl Debug for CronJob {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CronJob")
            .field("id", &self.id)
            .field("schedule", &self.schedule)
            .field("action_fn", &"<fn>")
            .finish()
    }
}
