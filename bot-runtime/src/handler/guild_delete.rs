use serenity::all::UnavailableGuild;
use sqlx::PgPool;
use tracing::info;
use zayden_app::guilds::GuildPresence;
use zayden_core::as_i64;

use super::Handler;
use crate::Result;

impl Handler {
    pub async fn guild_delete(
        &self,
        guild: &UnavailableGuild,
        pool: &PgPool,
    ) -> Result<()> {
        if guild.unavailable {
            return Ok(());
        }

        GuildPresence::left(pool, as_i64(guild.id.get()), self.me.application_i64())
            .await?;
        info!(guild_id = %guild.id, "Removed from guild; its data is kept for the retention window");

        Ok(())
    }
}
