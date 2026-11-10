use std::time::Duration;

use sqlx::PgPool;
use tracing::{error, warn};
use zayden_app::applications::{ApplicationModules, ProcessStatus};
use zayden_core::Identity;

use crate::modules::ModuleSet;

pub(crate) async fn record_start(pool: &PgPool, me: Identity, modules: &ModuleSet) {
    if let Err(e) =
        ApplicationModules::replace(pool, me.application_i64(), &modules.ids()).await
    {
        error!(error = ?e, "failed to record the application module list");
    }

    let process = me.application_id.to_string();
    if let Err(e) = ProcessStatus::started(
        pool,
        &process,
        Some(me.application_i64()),
        &build_sha(),
    )
    .await
    {
        error!(error = ?e, "failed to record the process start");
    }
}

pub(crate) fn spawn_heartbeat(pool: PgPool, me: Identity) {
    tokio::spawn(async move {
        let process = me.application_id.to_string();
        let mut interval = tokio::time::interval(Duration::from_secs(60));
        interval.tick().await;

        loop {
            interval.tick().await;
            if let Err(e) = ProcessStatus::seen(
                &pool,
                &process,
                Some(me.application_i64()),
                &build_sha(),
            )
            .await
            {
                warn!(error = ?e, "failed to record the process heartbeat");
            }
        }
    });
}

fn build_sha() -> String {
    std::env::var("BUILD_SHA").unwrap_or_else(|_| "dev".to_owned())
}
