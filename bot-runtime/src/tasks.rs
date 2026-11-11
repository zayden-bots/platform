use std::sync::Arc;
use std::time::Duration;

use futures::future::join_all;
use jiff::Timestamp;
use serenity::all::{
    ChannelId,
    Context,
    HttpError,
    JsonErrorCode,
    MessageId,
    StatusCode,
    UserId,
};
use tokio::sync::broadcast::error::RecvError;
use tokio::sync::broadcast::{self};
use tracing::{error, warn};
use zayden_app::events::AppEvent;
use zayden_app::state::AppState;
use zayden_app::tasks::{PLATFORM_STALE_AFTER, ScheduledTasks, Task};
use zayden_core::{
    DM_TASK,
    DmPayload,
    HandlerError,
    Identity,
    POST_TASK,
    PostPayload,
    TaskCtx,
};

use crate::modules::ModuleSet;

const BATCH: i64 = 25;
const IDLE: Duration = Duration::from_secs(60);
const RETRY: Duration = Duration::from_secs(30);
const MIN_IDLE: Duration = Duration::from_secs(1);
const TASK_TIMEOUT: Duration = Duration::from_secs(240);

pub(crate) fn spawn_task_runner(
    ctx: Context,
    app: Arc<AppState>,
    modules: Arc<ModuleSet>,
    me: Identity,
) {
    let mut kinds = modules.task_kinds();
    kinds.extend([POST_TASK, DM_TASK]);

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
    modules: &Arc<ModuleSet>,
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

    let spawned = batch.into_iter().map(|task| {
        let ctx = ctx.clone();
        let app = Arc::clone(app);
        let modules = Arc::clone(modules);
        let owned = task.clone();
        let handle = tokio::spawn(async move {
            tokio::time::timeout(TASK_TIMEOUT, async {
                if matches!(owned.kind.as_str(), POST_TASK | DM_TASK) {
                    return Some(run_platform(&ctx, &owned).await);
                }
                let cx = TaskCtx { ctx: &ctx, app, me };
                modules.run_task(&cx, &owned).await
            })
            .await
        });
        (task, handle)
    });

    join_all(
        spawned.map(|(task, handle)| async move {
            match handle.await {
                Ok(Ok(Some(Ok(())))) => {},
                Ok(Ok(Some(Err(e)))) => {
                    error!(
                        kind = task.kind,
                        key = task.key,
                        error = ?e,
                        "scheduled task failed",
                    );
                },
                Ok(Ok(None)) => {
                    error!(
                        kind = task.kind,
                        key = task.key,
                        "no module runs this scheduled task kind",
                    );
                },
                Ok(Err(_)) => {
                    error!(
                        kind = task.kind,
                        key = task.key,
                        "scheduled task timed out",
                    );
                },
                Err(e) => {
                    error!(
                        kind = task.kind,
                        key = task.key,
                        error = ?e,
                        "scheduled task panicked",
                    );
                },
            }
            complete(app, &task).await;
        }),
    )
    .await;

    Ok(full)
}

async fn run_platform(ctx: &Context, task: &Task) -> Result<(), HandlerError> {
    if Timestamp::now().duration_since(task.run_at) > PLATFORM_STALE_AFTER {
        warn!(kind = task.kind, key = task.key, "platform task dropped as stale");
        return Ok(());
    }

    let sent = if task.kind == POST_TASK {
        let payload = task.payload::<PostPayload>().map_err(HandlerError::new)?;
        send_post(ctx, &payload).await
    } else {
        let payload = task.payload::<DmPayload>().map_err(HandlerError::new)?;
        send_dm(ctx, &payload).await
    };

    if let Err(e) = sent {
        warn!(
            kind = task.kind,
            key = task.key,
            error = ?e,
            "platform task failed",
        );
    }
    Ok(())
}

async fn send_post(ctx: &Context, payload: &PostPayload) -> serenity::Result<()> {
    let channel = ChannelId::new(payload.channel_id).widen();

    if let Some(message_id) = payload.edit_message_id {
        match ctx
            .http
            .edit_message(
                channel,
                MessageId::new(message_id),
                &payload.message,
                Vec::new(),
            )
            .await
        {
            Ok(_) => return Ok(()),
            Err(e) if is_missing_message(&e) => {},
            Err(e) => return Err(e),
        }
    }

    ctx.http.send_message(channel, Vec::new(), &payload.message).await?;
    Ok(())
}

async fn send_dm(ctx: &Context, payload: &DmPayload) -> serenity::Result<()> {
    let dm = UserId::new(payload.user_id).create_dm_channel(&ctx.http).await?;
    ctx.http.send_message(dm.id.widen(), Vec::new(), &payload.message).await?;
    Ok(())
}

fn is_missing_message(e: &serenity::Error) -> bool {
    let serenity::Error::Http(HttpError::UnsuccessfulRequest(response)) = e else {
        return false;
    };
    response.error.code == JsonErrorCode::UnknownMessage
        || response.status_code == StatusCode::FORBIDDEN
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
            .clamp(MIN_IDLE, IDLE),
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
