use sqlx::PgPool;

use crate::applications::SealedToken;

#[derive(Debug, Clone)]
pub struct StoredHook {
    pub guild_id: i64,
    pub webhook_id: i64,
    pub sealed: SealedToken,
    pub sender_name: Option<String>,
    pub sender_avatar_url: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GuildHook {
    pub channel_id: i64,
    pub webhook_id: i64,
    pub broken: bool,
}

#[derive(Debug, Clone)]
pub struct NewHook {
    pub channel_id: i64,
    pub application_id: i64,
    pub guild_id: i64,
    pub webhook_id: i64,
    pub sealed: SealedToken,
}

pub struct DeliveryStore;

impl DeliveryStore {
    pub async fn hook(
        pool: &PgPool,
        channel_id: i64,
        application_id: i64,
    ) -> sqlx::Result<Option<StoredHook>> {
        let row = sqlx::query!(
            "SELECT w.guild_id, w.webhook_id, w.token_ct, w.token_nonce, w.key_id,
                    a.name AS \"name?\", a.avatar, a.bot_user_id AS \"bot_user_id?\"
             FROM delivery_webhooks w
             LEFT JOIN applications a ON a.application_id = w.application_id
             WHERE w.channel_id = $1
               AND w.application_id = $2
               AND w.broken_at IS NULL",
            channel_id,
            application_id,
        )
        .fetch_optional(pool)
        .await?;

        Ok(row.map(|row| StoredHook {
            guild_id: row.guild_id,
            webhook_id: row.webhook_id,
            sealed: SealedToken {
                key_id: row.key_id,
                nonce: row.token_nonce,
                ciphertext: row.token_ct,
            },
            sender_name: row.name.filter(|name| allowed_sender_name(name)),
            sender_avatar_url: avatar_url(row.bot_user_id, row.avatar.as_deref()),
        }))
    }

    pub async fn upsert(pool: &PgPool, hook: &NewHook) -> sqlx::Result<()> {
        sqlx::query!(
            "INSERT INTO delivery_webhooks
                 (channel_id, application_id, guild_id, webhook_id,
                  token_ct, token_nonce, key_id)
             VALUES ($1, $2, $3, $4, $5, $6, $7)
             ON CONFLICT (channel_id, application_id) DO UPDATE SET
                 guild_id = EXCLUDED.guild_id,
                 webhook_id = EXCLUDED.webhook_id,
                 token_ct = EXCLUDED.token_ct,
                 token_nonce = EXCLUDED.token_nonce,
                 key_id = EXCLUDED.key_id,
                 broken_at = NULL,
                 created_at = now()",
            hook.channel_id,
            hook.application_id,
            hook.guild_id,
            hook.webhook_id,
            hook.sealed.ciphertext,
            hook.sealed.nonce,
            hook.sealed.key_id,
        )
        .execute(pool)
        .await?;

        Ok(())
    }

    pub async fn mark_broken(
        pool: &PgPool,
        channel_id: i64,
        application_id: i64,
        webhook_id: i64,
    ) -> sqlx::Result<bool> {
        let result = sqlx::query!(
            "UPDATE delivery_webhooks SET broken_at = now()
             WHERE channel_id = $1
               AND application_id = $2
               AND webhook_id = $3
               AND broken_at IS NULL",
            channel_id,
            application_id,
            webhook_id,
        )
        .execute(pool)
        .await?;

        Ok(result.rows_affected() > 0)
    }

    pub async fn delete(
        pool: &PgPool,
        channel_id: i64,
        application_id: i64,
        webhook_id: i64,
    ) -> sqlx::Result<bool> {
        let result = sqlx::query!(
            "DELETE FROM delivery_webhooks
             WHERE channel_id = $1 AND application_id = $2 AND webhook_id = $3",
            channel_id,
            application_id,
            webhook_id,
        )
        .execute(pool)
        .await?;

        Ok(result.rows_affected() > 0)
    }

    pub async fn for_guild(
        pool: &PgPool,
        guild_id: i64,
        application_id: i64,
    ) -> sqlx::Result<Vec<GuildHook>> {
        sqlx::query_as!(
            GuildHook,
            "SELECT channel_id, webhook_id, broken_at IS NOT NULL AS \"broken!\"
             FROM delivery_webhooks
             WHERE guild_id = $1 AND application_id = $2
             ORDER BY channel_id",
            guild_id,
            application_id,
        )
        .fetch_all(pool)
        .await
    }

    pub async fn channel_hook(
        pool: &PgPool,
        channel_id: i64,
        application_id: i64,
    ) -> sqlx::Result<Option<GuildHook>> {
        sqlx::query_as!(
            GuildHook,
            "SELECT channel_id, webhook_id, broken_at IS NOT NULL AS \"broken!\"
             FROM delivery_webhooks
             WHERE channel_id = $1 AND application_id = $2",
            channel_id,
            application_id,
        )
        .fetch_optional(pool)
        .await
    }

    pub async fn wanted(
        pool: &PgPool,
        guild_id: i64,
        application_id: i64,
    ) -> sqlx::Result<Vec<i64>> {
        sqlx::query_scalar!(
            "SELECT DISTINCT t.channel_id AS \"channel_id!\"
             FROM delivery_targets t
             WHERE t.guild_id = $1
               AND delivery_application(t.guild_id, t.delivery_application_id) = $2
             ORDER BY 1",
            guild_id,
            application_id,
        )
        .fetch_all(pool)
        .await
    }

    pub async fn wanted_guilds(
        pool: &PgPool,
        application_id: i64,
    ) -> sqlx::Result<Vec<i64>> {
        sqlx::query_scalar!(
            "SELECT g AS \"g!\" FROM (
                 SELECT t.guild_id AS g
                 FROM delivery_targets t
                 WHERE delivery_application(t.guild_id, t.delivery_application_id) = $1
                 UNION
                 SELECT w.guild_id AS g
                 FROM delivery_webhooks w
                 WHERE w.application_id = $1
             ) s
             ORDER BY g",
            application_id,
        )
        .fetch_all(pool)
        .await
    }
}

fn allowed_sender_name(name: &str) -> bool {
    let lower = name.to_lowercase();
    !lower.contains("clyde") && !lower.contains("discord")
}

fn avatar_url(bot_user_id: Option<i64>, avatar: Option<&str>) -> Option<String> {
    let (user, avatar) = bot_user_id.zip(avatar)?;
    Some(format!("https://cdn.discordapp.com/avatars/{user}/{avatar}.png"))
}
