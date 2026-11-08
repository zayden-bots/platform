use serenity::all::{Context, Ready};
use sqlx::PgPool;
use tokio::sync::RwLock;
use zayden_core::{EmojiCache, RuntimeState};

use crate::{Result, ZAYDEN_TOKEN, zayden_token};

pub async fn load_emojis(
    ctx: &Context,
    ready: &Ready,
    pool: &PgPool,
    zayden_id: u64,
) -> Result<()> {
    let cache = if ready.application.id.get() == zayden_id {
        EmojiCache::new(ctx).await?
    } else {
        let token = ZAYDEN_TOKEN.get_or_try_init(|| zayden_token(pool)).await?;
        EmojiCache::new_from_parent(ctx, token).await?
    };

    ctx.data::<RwLock<RuntimeState>>().write().await.set_emojis(cache);
    Ok(())
}
