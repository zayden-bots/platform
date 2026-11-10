use std::sync::Arc;
use std::time::Duration;

use futures::future::join_all;
use jiff::Timestamp;
use serenity::all::Context;
use tokio::sync::broadcast::error::RecvError;
use tokio::sync::broadcast::{self};
use tracing::{error, warn};
use zayden_app::events::AppEvent;
use zayden_app::state::AppState;
use zayden_app::tasks::{ScheduledTasks, Task};
use zayden_core::{Identity, TaskCtx};

use crate::modules::ModuleSet;

const BATCH: i64 = 25;
const IDLE: Duration = Duration::from_secs(60);
const RETRY: Duration = Duration::from_secs(30);

pub(crate) fn spawn_task_runner(
    ctx: Context,
    app: Arc<AppState>,
    modules: Arc<ModuleSet>,
    me: Identity,
) {
    let kinds = modules.task_kinds();
    if kinds.is_empty() {
        return;
    }

    let mut events = Some(app.subscribe());

    tokio::spawn(async move {
        let application_id = me.application_i64();

        loop {
            let delay = match poll(&ctx, &app, &modules, me, &kinds).await {
                Ok(true) => continue,
                Ok(false) => next_delay(&app, application_id, &kinds).await,
                Err(e) => {
                    warn!(error = ?e, "scheduled task poll failed");
                    RETRY
                },
            };

            sleep_until_woken(&mut events, me.application_id.get(), delay).await;
        }
    });
}

async fn poll(
    ctx: &Context,
    app: &Arc<AppState>,
    modules: &ModuleSet,
    me: Identity,
    kinds: &[&'static str],
) -> sqlx::Result<bool> {
    let application_id = me.application_i64();

    for task in ScheduledTasks::release_stale(&app.db, application_id).await? {
        error!(
            id = task.id,
            kind = task.kind,
            key = task.key,
            "scheduled task dropped after repeated claims",
        );
    }

    let batch =
        ScheduledTasks::claim_due(&app.db, application_id, kinds, BATCH).await?;
    let full = i64::try_from(batch.len()).is_ok_and(|len| len >= BATCH);

    join_all(batch.iter().map(|task| async move {
        let cx = TaskCtx { ctx, app: Arc::clone(app), me };
        match modules.run_task(&cx, task).await {
            Some(Ok(())) => {},
            Some(Err(e)) => {
                error!(
                    kind = task.kind,
                    key = task.key,
                    error = ?e,
                    "scheduled task failed",
                );
            },
            None => {
                error!(
                    kind = task.kind,
                    key = task.key,
                    "no module runs this scheduled task kind",
                );
            },
        }
        complete(app, task).await;
    }))
    .await;

    Ok(full)
}

async fn complete(app: &AppState, task: &Task) {
    if let Err(e) = ScheduledTasks::complete(&app.db, task).await {
        warn!(
            kind = task.kind,
            key = task.key,
            error = ?e,
            "failed to complete scheduled task",
        );
    }
}

async fn next_delay(
    app: &AppState,
    application_id: i64,
    kinds: &[&'static str],
) -> Duration {
    match ScheduledTasks::next_due(&app.db, application_id, kinds).await {
        Ok(Some(next)) => Duration::try_from(Timestamp::now().duration_until(next))
            .unwrap_or(Duration::ZERO)
            .min(IDLE),
        Ok(None) => IDLE,
        Err(e) => {
            warn!(error = ?e, "scheduled task poll failed");
            RETRY
        },
    }
}

async fn sleep_until_woken(
    events: &mut Option<broadcast::Receiver<AppEvent>>,
    application_id: u64,
    delay: Duration,
) {
    let sleep = tokio::time::sleep(delay);
    tokio::pin!(sleep);

    loop {
        let Some(rx) = events.as_mut() else {
            sleep.await;
            return;
        };

        tokio::select! {
            () = &mut sleep => return,
            event = rx.recv() => match event {
                Ok(AppEvent::TaskScheduled(id)) if id == application_id => return,
                Ok(AppEvent::Resync) | Err(RecvError::Lagged(_)) => return,
                Ok(_) => {},
                Err(RecvError::Closed) => *events = None,
            },
        }
    }
}
