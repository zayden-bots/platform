use serenity::all::{Context, Guild};
use sqlx::PgPool;
use tokio::sync::RwLock;
use tracing::{info, warn};
use zayden_app::guilds::{GuildOwner, GuildPresence, JoinKind};
use zayden_core::{RuntimeState, as_i64};

use super::Handler;
use crate::{Result, module_sync, serving_guard};

impl Handler {
    pub async fn guild_create(
        &self,
        ctx: &Context,
        guild: &Guild,
        pool: &PgPool,
    ) -> Result<()> {
        let joined_at = *guild.joined_at;

        let join = GuildPresence::joined(
            pool,
            as_i64(guild.id.get()),
            self.me.application_i64(),
            joined_at,
        )
        .await
        .unwrap_or_else(|e| {
            warn!(error = ?e, guild_id = %guild.id, "failed to record guild presence");
            JoinKind::Reconnect
        });

        match serving_guard::enforce(&ctx.http, &self.app, self.me, guild.id).await {
            Ok(true) => return Ok(()),
            Ok(false) => {},
            Err(e) => warn!(
                error = ?e,
                guild_id = %guild.id,
                "failed to check which application serves this guild"
            ),
        }

        if let Err(e) = GuildOwner::record(
            pool,
            as_i64(guild.id.get()),
            as_i64(guild.owner_id.get()),
        )
        .await
        {
            warn!(error = ?e, guild_id = %guild.id, "failed to record guild owner");
        }

        ctx.data::<RwLock<RuntimeState>>().read().await.guild_create(guild);

        let states = module_sync::seed(
            &ctx.http,
            &self.app,
            &self.registry,
            guild,
            joined_at,
            join,
            self.me.application_id,
        )
        .await?;

        module_sync::sync_commands(&ctx.http, &self.registry, guild.id, &states)
            .await?;
        info!("Registered {}", guild.name);

        Ok(())
    }
}
