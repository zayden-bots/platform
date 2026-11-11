use std::num::NonZeroU16;
use std::sync::Arc;

use serenity::all::{Context, OnlineStatus, Ready};
use tracing::{info, warn};
use zayden_app::guilds::{GuildPresence, Shard};
use zayden_core::{StartCtx, as_i64};

use crate::cron::{collect_jobs, start_cron_jobs};
use crate::handler::Handler;
use crate::state::load_emojis;
use crate::{Result, delivery, module_sync, serving_guard, tasks};

impl Handler {
    pub async fn ready(&self, ctx: &Context, ready: &Ready) -> Result<()> {
        info!(
            "{} is connected ({} shards) and in {} guilds!",
            ready.user.name,
            ready.shard.map_or(NonZeroU16::MIN, |info| info.total),
            ready.guilds.len()
        );

        ctx.set_presence(None, OnlineStatus::Online);

        let pool = self.app.db.clone();
        load_emojis(ctx, &pool, self.me).await?;
        self.reconcile_presence(ready, &pool).await;

        if self.cron_started.set(()).is_ok() {
            self.background_tasks(ctx, ready).await;
        }

        Ok(())
    }

    async fn reconcile_presence(&self, ready: &Ready, pool: &sqlx::PgPool) {
        let shard = ready.shard.map_or(Shard { id: 0, total: 1 }, |info| Shard {
            id: info.id.0,
            total: info.total.get(),
        });
        let present = ready
            .guilds
            .iter()
            .map(|guild| as_i64(guild.id.get()))
            .collect::<Vec<_>>();

        match GuildPresence::reconcile(
            pool,
            self.me.application_i64(),
            shard,
            &present,
        )
        .await
        {
            Ok(0) => {},
            Ok(marked) => {
                info!(marked, "guilds removed while offline enter retention");
            },
            Err(e) => warn!(error = ?e, "failed to reconcile guild presence"),
        }
    }

    async fn background_tasks(&self, ctx: &Context, ready: &Ready) {
        {
            let mut state = self.bot_state.write().await;
            state.jobs_mut().extend(collect_jobs(self.modules.entity_jobs(self.me)));
        }

        self.modules.start(&StartCtx {
            ctx,
            app: Arc::clone(&self.app),
            ready,
            me: self.me,
        });

        if let Some(handle) =
            delivery::spawn(Arc::clone(&ctx.http), Arc::clone(&self.app), self.me)
        {
            let _ = self.delivery.set(handle);
        }

        module_sync::spawn_listener(
            Arc::clone(&ctx.http),
            Arc::clone(&self.app),
            Arc::clone(&self.registry),
            self.me.application_id,
        );
        serving_guard::spawn_listener(
            Arc::clone(&ctx.http),
            Arc::clone(&self.app),
            self.me,
        );

        tasks::spawn_task_runner(
            ctx.clone(),
            Arc::clone(&self.app),
            Arc::clone(&self.modules),
            self.me,
        );

        let ctx = ctx.clone();
        let pool = self.app.db.clone();
        tokio::spawn(async move { start_cron_jobs(ctx, pool).await });
    }
}
