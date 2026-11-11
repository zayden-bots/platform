use sqlx::PgPool;

use super::DeliveryStore;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeliveryKind {
    Youtube,
    Patreon,
    DeadlockPatch,
    DeadlockSummary,
    DeadlockSquad,
}

impl DeliveryStore {
    pub async fn bound_application(
        pool: &PgPool,
        kind: DeliveryKind,
        guild_id: i64,
    ) -> sqlx::Result<Option<i64>> {
        let bound = match kind {
            DeliveryKind::Youtube => {
                sqlx::query_scalar!(
                    "SELECT delivery_application_id FROM youtube_announce
                     WHERE guild_id = $1",
                    guild_id,
                )
                .fetch_optional(pool)
                .await?
            },
            DeliveryKind::Patreon => {
                sqlx::query_scalar!(
                    "SELECT delivery_application_id FROM patreon_announce
                     WHERE guild_id = $1",
                    guild_id,
                )
                .fetch_optional(pool)
                .await?
            },
            DeliveryKind::DeadlockPatch => {
                sqlx::query_scalar!(
                    "SELECT delivery_application_id FROM deadlock_patch_announce
                     WHERE guild_id = $1",
                    guild_id,
                )
                .fetch_optional(pool)
                .await?
            },
            DeliveryKind::DeadlockSummary => {
                sqlx::query_scalar!(
                    "SELECT delivery_application_id FROM deadlock_summary_channel
                     WHERE guild_id = $1",
                    guild_id,
                )
                .fetch_optional(pool)
                .await?
            },
            DeliveryKind::DeadlockSquad => {
                sqlx::query_scalar!(
                    "SELECT delivery_application_id FROM deadlock_squad_channel
                     WHERE guild_id = $1",
                    guild_id,
                )
                .fetch_optional(pool)
                .await?
            },
        };

        Ok(bound.flatten())
    }

    pub async fn bind_application(
        pool: &PgPool,
        kind: DeliveryKind,
        guild_id: i64,
        application_id: Option<i64>,
    ) -> sqlx::Result<bool> {
        let result = match kind {
            DeliveryKind::Youtube => {
                sqlx::query!(
                    "UPDATE youtube_announce SET delivery_application_id = $2
                     WHERE guild_id = $1
                       AND delivery_application_id IS DISTINCT FROM $2",
                    guild_id,
                    application_id,
                )
                .execute(pool)
                .await?
            },
            DeliveryKind::Patreon => {
                sqlx::query!(
                    "UPDATE patreon_announce SET delivery_application_id = $2
                     WHERE guild_id = $1
                       AND delivery_application_id IS DISTINCT FROM $2",
                    guild_id,
                    application_id,
                )
                .execute(pool)
                .await?
            },
            DeliveryKind::DeadlockPatch => {
                sqlx::query!(
                    "UPDATE deadlock_patch_announce SET delivery_application_id = $2
                     WHERE guild_id = $1
                       AND delivery_application_id IS DISTINCT FROM $2",
                    guild_id,
                    application_id,
                )
                .execute(pool)
                .await?
            },
            DeliveryKind::DeadlockSummary => {
                sqlx::query!(
                    "UPDATE deadlock_summary_channel SET delivery_application_id = $2
                     WHERE guild_id = $1
                       AND delivery_application_id IS DISTINCT FROM $2",
                    guild_id,
                    application_id,
                )
                .execute(pool)
                .await?
            },
            DeliveryKind::DeadlockSquad => {
                sqlx::query!(
                    "UPDATE deadlock_squad_channel SET delivery_application_id = $2
                     WHERE guild_id = $1
                       AND delivery_application_id IS DISTINCT FROM $2",
                    guild_id,
                    application_id,
                )
                .execute(pool)
                .await?
            },
        };

        Ok(result.rows_affected() > 0)
    }

    pub async fn effective_application(
        pool: &PgPool,
        guild_id: i64,
        bound: Option<i64>,
    ) -> sqlx::Result<Option<i64>> {
        sqlx::query_scalar!(
            "SELECT delivery_application($1, $2) AS application_id",
            guild_id,
            bound,
        )
        .fetch_one(pool)
        .await
    }
}
