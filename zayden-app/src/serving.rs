use std::sync::Arc;
use std::time::Duration;

use moka::future::Cache;
use sqlx::PgPool;
use tokio::sync::broadcast::Receiver;
use tokio::sync::broadcast::error::RecvError;
use tracing::warn;

use crate::applications::Slot;
use crate::events::AppEvent;

const CACHE_TTL: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServingKind {
    Default,
    Custom,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ServingBot {
    pub application_id: u64,
    pub kind: ServingKind,
    pub present: bool,
}

pub struct ServingResolver {
    db: PgPool,
    cache: Cache<(i64, Slot), Option<ServingBot>>,
}

impl ServingResolver {
    #[must_use]
    pub fn new(db: PgPool) -> Self {
        let cache =
            Cache::builder().max_capacity(4_096).time_to_live(CACHE_TTL).build();
        Self { db, cache }
    }

    pub async fn resolve(
        &self,
        guild_id: i64,
        slot: Slot,
    ) -> sqlx::Result<Option<ServingBot>> {
        if let Some(cached) = self.cache.get(&(guild_id, slot)).await {
            return Ok(cached);
        }

        self.fetch(guild_id, slot).await
    }

    pub async fn fetch(
        &self,
        guild_id: i64,
        slot: Slot,
    ) -> sqlx::Result<Option<ServingBot>> {
        let row = sqlx::query!(
            r#"WITH s AS (
                   SELECT custom_application($1, $2) AS custom,
                          (SELECT application_id FROM applications
                           WHERE slot = $2 AND kind = 'default') AS fallback
               )
               SELECT s.custom, s.fallback,
                      EXISTS (
                          SELECT 1 FROM guild_presence p
                          WHERE p.guild_id = $1
                            AND p.application_id = COALESCE(s.custom, s.fallback)
                            AND p.left_at IS NULL
                      ) AS "present!"
               FROM s"#,
            guild_id,
            slot.as_str(),
        )
        .fetch_one(&self.db)
        .await?;

        let bot = match (row.custom, row.fallback) {
            (Some(custom), _) => Some((custom, ServingKind::Custom)),
            (None, Some(fallback)) => Some((fallback, ServingKind::Default)),
            (None, None) => None,
        }
        .map(|(application_id, kind)| ServingBot {
            application_id: application_id.cast_unsigned(),
            kind,
            present: row.present,
        });

        self.cache.insert((guild_id, slot), bot).await;
        Ok(bot)
    }

    pub fn spawn_invalidator(this: &Arc<Self>, mut rx: Receiver<AppEvent>) {
        let weak = Arc::downgrade(this);

        tokio::spawn(async move {
            loop {
                let event = rx.recv().await;
                let Some(this) = weak.upgrade() else { break };
                match event {
                    Ok(AppEvent::ServingChanged(guild_id)) => {
                        for slot in Slot::ALL {
                            this.cache
                                .invalidate(&(guild_id.cast_signed(), slot))
                                .await;
                        }
                    },
                    Ok(AppEvent::CustomBotsChanged(_) | AppEvent::Resync) => {
                        this.cache.invalidate_all();
                    },
                    Ok(_) => {},
                    Err(RecvError::Lagged(n)) => {
                        warn!(n, "serving invalidator lagged; clearing full cache");
                        this.cache.invalidate_all();
                    },
                    Err(RecvError::Closed) => break,
                }
            }
        });
    }
}
