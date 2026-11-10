use serenity::all::Context;
use sqlx::PgPool;
use tokio::sync::RwLock;
use zayden_core::{EmojiCache, Identity, RuntimeState};

use crate::{Result, ZAYDEN_TOKEN, zayden_token};

pub async fn load_emojis(ctx: &Context, pool: &PgPool, me: Identity) -> Result<()> {
    let cache = if me.is_primary() {
        EmojiCache::new(ctx).await?
    } else {
        let token = ZAYDEN_TOKEN.get_or_try_init(|| zayden_token(pool)).await?;
        EmojiCache::new_from_parent(ctx, token).await?
    };

    ctx.data::<RwLock<RuntimeState>>().write().await.set_emojis(cache);
    Ok(())
}
