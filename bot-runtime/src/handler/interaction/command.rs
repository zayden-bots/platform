use std::sync::Arc;

use serenity::all::{CommandInteraction, Context};
use tracing::{info, warn};
use zayden_app::state::AppState;
use zayden_core::{Identity, get_option_str};

use super::respond_with_error;
use crate::CommandRegistry;
use crate::handler::Handler;

impl Handler {
    pub async fn interaction_command(
        ctx: &Context,
        interaction: &CommandInteraction,
        app: Arc<AppState>,
        registry: Arc<CommandRegistry>,
        me: Identity,
    ) {
        let options = interaction.data.options();

        info!(
            "{} ran command: {}{}",
            interaction.user.name,
            interaction.data.name,
            get_option_str(&options)
        );

        match registry.run_command(ctx, interaction, app, me).await {
            Some(Ok(())) => {},
            Some(Err(e)) => respond_with_error(ctx, interaction, e).await,
            None => {
                warn!(command = interaction.data.name.as_str(), "unknown command");
            },
        }
    }
}
