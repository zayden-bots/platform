use serenity::all::{ApplicationId, Context};
use tokio::sync::RwLock;
use tracing::warn;
use zayden_app::state::AppState;
use zayden_core::{EmojiCache, Identity, RuntimeState};

use crate::Result;

pub async fn load_emojis(ctx: &Context, app: &AppState, me: Identity) -> Result<()> {
    let parent = ApplicationId::new(app.zayden_id);
    let cache = if me.is_primary() {
        EmojiCache::new(ctx).await?.with_parent(parent)
    } else {
        match crate::parent_token(app, me).await {
            Ok(token) => {
                EmojiCache::new_from_parent(ctx, parent, &token).await?
            },
            Err(e) => {
                warn!(
                    error = %e,
                    "cannot read Zayden's token: application emojis are not copied"
                );
                EmojiCache::new(ctx).await?.with_parent(parent)
            },
        }
    };

    ctx.data::<RwLock<RuntimeState>>().write().await.set_emojis(cache);
    Ok(())
}
