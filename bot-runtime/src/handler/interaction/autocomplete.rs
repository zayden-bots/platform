use std::sync::Arc;

use serenity::all::{CommandInteraction, Context};
use tracing::{error, warn};
use zayden_app::state::AppState;
use zayden_core::Identity;

use crate::handler::Handler;
use crate::{CommandRegistry, Result};

impl Handler {
    pub async fn interaction_autocomplete(
        ctx: &Context,
        interaction: &CommandInteraction,
        app: Arc<AppState>,
        registry: Arc<CommandRegistry>,
        me: Identity,
    ) -> Result<()> {
        match registry.run_autocomplete(ctx, interaction, app, me).await {
            Some(Ok(())) => {},
            Some(Err(err)) => {
                error!(
                    error = ?err,
                    command = interaction.data.name.as_str(),
                    user = interaction.user.name.as_str(),
                    "autocomplete handler error",
                );
            },
            None => warn!("Unknown autocomplete command: {}", interaction.data.name),
        }

        Ok(())
    }
}
