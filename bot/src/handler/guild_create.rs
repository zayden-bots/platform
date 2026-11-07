use serenity::all::{Context, Guild};
use sqlx::PgPool;
use tokio::sync::RwLock;
use tracing::{info, warn};
use zayden_app::guilds::{GuildOwner, GuildPresence, JoinKind};
use zayden_core::as_i64;

use super::Handler;
use crate::{BotState, Result, module_sync};

impl Handler {
    pub async fn guild_create(
        &self,
        ctx: &Context,
        guild: &Guild,
        pool: &PgPool,
    ) -> Result<()> {
        let joined_at = *guild.joined_at;

        let join = match ctx.http.application_id() {
            Some(application_id) => GuildPresence::joined(
                pool,
                as_i64(guild.id.get()),
                as_i64(application_id.get()),
                joined_at,
            )
            .await
            .unwrap_or_else(|e| {
                warn!(error = ?e, guild_id = %guild.id, "failed to record guild presence");
                JoinKind::Reconnect
            }),
            None => {
                warn!(guild_id = %guild.id, "guild seen before the application id");
                JoinKind::Reconnect
            },
        };

        if let Err(e) = GuildOwner::record(
            pool,
            as_i64(guild.id.get()),
            as_i64(guild.owner_id.get()),
        )
        .await
        {
            warn!(error = ?e, guild_id = %guild.id, "failed to record guild owner");
        }

        let data = ctx.data::<RwLock<BotState>>();

        BotState::guild_create(data, guild).await;

        let states = module_sync::seed(
            &ctx.http,
            &self.app,
            &self.registry,
            guild,
            joined_at,
            join,
        )
        .await?;

        module_sync::sync_commands(&ctx.http, &self.registry, guild.id, &states)
            .await?;
        info!("Registered {}", guild.name);

        Ok(())
    }
}
