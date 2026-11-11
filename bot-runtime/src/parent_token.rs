use secrecy::ExposeSecret;
use tracing::{info, warn};
use zayden_app::applications::{ApplicationError, ApplicationStore};
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
    if !me.is_primary() {
        return;
    }

    let Some(keyring) = app.keyring.as_deref() else {
        warn!(
            "no keyring: Zayden's token is not published, so other bots cannot copy its emojis"
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
        Ok(true) => info!("Zayden's token was sealed into its application row"),
        Ok(false) => {},
        Err(e) => warn!(error = %e, "failed to publish Zayden's token"),
    }
}
