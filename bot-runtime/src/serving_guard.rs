use std::sync::Arc;

use serenity::all::{GuildId, Http, UserId};
use serenity::http::StatusCode;
use tokio::sync::broadcast::error::RecvError;
use tracing::{info, warn};
use zayden_app::applications::{AppKind, ApplicationStore};
use zayden_app::events::AppEvent;
use zayden_app::guilds::GuildPresence;
use zayden_app::serving::ServingKind;
use zayden_app::state::AppState;
use zayden_core::{Identity, as_i64};

use crate::Result;

pub(crate) async fn enforce(
    http: &Http,
    app: &AppState,
    me: Identity,
    guild_id: GuildId,
) -> Result<bool> {
    if me.kind != AppKind::Default {
        return Ok(false);
    }

    let Some(bot) = app.serving.fetch(as_i64(guild_id.get()), me.slot).await? else {
        return Ok(false);
    };
    if bot.kind != ServingKind::Custom || !bot.present {
        return Ok(false);
    }

    let Some(bot_user_id) =
        ApplicationStore::bot_user_id(&app.db, as_i64(bot.application_id)).await?
    else {
        return Ok(false);
    };
    match guild_id.member(http, UserId::new(bot_user_id.cast_unsigned())).await {
        Err(serenity::Error::Http(e))
            if e.status_code() == Some(StatusCode::NOT_FOUND) =>
        {
            info!(
                guild_id = %guild_id,
                custom_application_id = bot.application_id,
                "custom application is recorded present but is not a member; staying"
            );
            return Ok(false);
        },
        Err(e) => return Err(e.into()),
        Ok(_) => {},
    }

    if app.auto_leave {
        guild_id.leave(http).await?;
        info!(
            guild_id = %guild_id,
            custom_application_id = bot.application_id,
            "left a guild served by a custom application"
        );
        Ok(true)
    } else {
        info!(
            guild_id = %guild_id,
            custom_application_id = bot.application_id,
            "custom application serves this guild; auto-leave is off"
        );
        Ok(false)
    }
}

pub(crate) fn spawn_listener(http: Arc<Http>, app: Arc<AppState>, me: Identity) {
    if me.kind != AppKind::Default {
        return;
    }

    tokio::spawn(async move {
        let mut rx = app.subscribe();

        loop {
            match rx.recv().await {
                Ok(AppEvent::ServingChanged(guild)) => {
                    let present = GuildPresence::is_present(
                        &app.db,
                        as_i64(guild),
                        me.application_i64(),
                    )
                    .await;
                    match present {
                        Ok(true) => {},
                        Ok(false) => continue,
                        Err(e) => {
                            warn!(
                                error = ?e,
                                guild_id = guild,
                                "failed to read presence before enforcing serving"
                            );
                            continue;
                        },
                    }

                    if let Err(e) =
                        enforce(&http, &app, me, GuildId::new(guild)).await
                    {
                        warn!(
                            error = ?e,
                            guild_id = guild,
                            "failed to enforce serving after a change"
                        );
                    }
                },
                Ok(_) => {},
                Err(RecvError::Lagged(n)) => {
                    warn!(
                        n,
                        "serving guard lagged; changes may be missed until restart"
                    );
                },
                Err(RecvError::Closed) => break,
            }
        }
    });
}
