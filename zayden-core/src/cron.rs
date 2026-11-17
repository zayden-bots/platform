use std::cmp::Ordering;
use std::fmt::Debug;
use std::pin::Pin;
use std::str::FromStr;
use std::sync::Arc;

use jiff::Timestamp;
use jiff_cron::{Schedule, jiff as cron_jiff};
use serenity::all::Context;
use sqlx::PgPool;
use zayden_app::state::AppState;

use crate::delivery::WebhookPool;

pub type ActionFn = Arc<
    dyn Fn(Context, PgPool) -> Pin<Box<dyn Future<Output = ()> + Send>>
        + Send
        + Sync,
>;

pub type CronResult = Result<CronJob, jiff_cron::error::Error>;

#[derive(Clone)]
pub struct CronCtx {
    pub db: PgPool,
    pub app: Arc<AppState>,
    pub webhooks: Arc<WebhookPool>,
}

pub type WorkerActionFn =
    Arc<dyn Fn(CronCtx) -> Pin<Box<dyn Future<Output = ()> + Send>> + Send + Sync>;

pub type WorkerJobResult = Result<WorkerJob, jiff_cron::error::Error>;

fn to_cron(at: Timestamp) -> Option<cron_jiff::Zoned> {
    cron_jiff::Timestamp::from_nanosecond(at.as_nanosecond())
        .ok()
        .map(|t| t.to_zoned(cron_jiff::tz::TimeZone::UTC))
}

fn from_cron(at: &cron_jiff::Zoned) -> Option<Timestamp> {
    Timestamp::from_nanosecond(at.timestamp().as_nanosecond()).ok()
}

pub trait Scheduled {
    type Action: Clone;

    fn id(&self) -> &str;
    fn schedule(&self) -> &Schedule;
    fn action(&self) -> Self::Action;

    // `includes` guards jiff-cron's `after` fast path, which can skip the
    // day-of-week check.
    fn next_run(&self, now: Timestamp) -> Option<Timestamp> {
        let schedule = self.schedule();
        schedule
            .after(to_cron(now)?)
            .find(|t| schedule.includes(t.clone()))
            .as_ref()
            .and_then(from_cron)
    }
}

impl Scheduled for CronJob {
    type Action = ActionFn;

    fn id(&self) -> &str {
        &self.id
    }

    fn schedule(&self) -> &Schedule {
        &self.schedule
    }

    fn action(&self) -> ActionFn {
        Arc::clone(&self.action_fn)
    }
}

impl Scheduled for WorkerJob {
    type Action = WorkerActionFn;

    fn id(&self) -> &str {
        &self.id
    }

    fn schedule(&self) -> &Schedule {
        &self.schedule
    }

    fn action(&self) -> WorkerActionFn {
        Arc::clone(&self.action_fn)
    }
}

pub struct PendingJob<J: Scheduled = CronJob> {
    pub at: Timestamp,
    pub id: String,
    pub action: J::Action,
}

#[derive(Clone)]
pub struct WorkerJob {
    pub id: String,
    pub schedule: Schedule,
    pub action_fn: WorkerActionFn,
}

impl WorkerJob {
    pub fn new(
        id: impl Into<String>,
        source: &str,
    ) -> Result<Self, jiff_cron::error::Error> {
        Ok(Self {
            id: id.into(),
            schedule: Schedule::from_str(source)?,
            action_fn: Self::action_fn(|_| async {}),
        })
    }

    fn action_fn<F, Fut>(f: F) -> WorkerActionFn
    where
        F: Fn(CronCtx) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = ()> + Send + 'static,
    {
        Arc::new(move |cx| Box::pin(f(cx)))
    }

    #[must_use]
    pub fn set_action<F, Fut>(mut self, f: F) -> Self
    where
        F: Fn(CronCtx) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = ()> + Send + 'static,
    {
        self.action_fn = Self::action_fn(f);
        self
    }
}

impl Debug for WorkerJob {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WorkerJob")
            .field("id", &self.id)
            .field("schedule", &self.schedule)
            .field("action_fn", &"<fn>")
            .finish()
    }
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

pub fn prune_exhausted<J: Scheduled>(jobs: &mut Vec<J>, now: Timestamp) {
    let start = to_cron(now);
    jobs.retain(|job| {
        start
            .as_ref()
            .is_none_or(|s| job.schedule().after(s.clone()).next().is_some())
    });
}

#[must_use]
pub fn earliest_pending<J: Scheduled>(
    jobs: &[J],
    now: Timestamp,
) -> Vec<PendingJob<J>> {
    let mut pending: Vec<PendingJob<J>> = Vec::new();

    for job in jobs {
        let Some(run_time) = job.next_run(now) else {
            continue;
        };

        let entry = PendingJob {
            at: run_time,
            id: job.id().to_owned(),
            action: job.action(),
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
