use secrecy::ExposeSecret;
use tracing::{info, warn};
use zayden_app::applications::{AppKind, ApplicationError, ApplicationStore};
use zayden_app::state::AppState;
use zayden_core::{Identity, as_i64};

pub async fn parent_token(
    app: &AppState,
    me: Identity,
) -> Result<String, ApplicationError> {
    if me.is_primary() {
        return Ok(app.discord_token.expose_secret().to_owned());
    }

    let keyring = app.keyring.as_deref().ok_or(ApplicationError::NoKeyring)?;
    let token =
        ApplicationStore::open_token(&app.db, keyring, as_i64(app.zayden_id))
            .await?;
    Ok(token.expose_secret().to_owned())
}

pub(crate) async fn publish(app: &AppState, me: Identity) {
    if !matches!(me.kind, AppKind::Default) {
        return;
    }

    let Some(keyring) = app.keyring.as_deref() else {
        warn!(
            slot = me.slot.as_str(),
            "no keyring: this bot's token is not published, so the dashboard and other bots cannot use it"
        );
        return;
    };

    match ApplicationStore::publish_default_token(
        &app.db,
        keyring,
        me.application_i64(),
        &app.discord_token,
    )
    .await
    {
        Ok(true) => info!(
            slot = me.slot.as_str(),
            "this bot's token was sealed into its application row"
        ),
        Ok(false) => {},
        Err(e) => warn!(
            slot = me.slot.as_str(),
            error = %e,
            "failed to publish this bot's token"
        ),
    }
}
