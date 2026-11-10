use std::borrow::Cow;
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use serenity::all::{
    CommandInteraction,
    ComponentInteraction,
    Context,
    CreateCommand,
    CreateInteractionResponse,
    CreateInteractionResponseFollowup,
    CreateInteractionResponseMessage,
    GuildId,
    MessageFlags,
    ModalInteraction,
};
use tracing::field::Empty;
use tracing::{Instrument, info_span, warn};
use zayden_app::entitlement::{EntitlementScope, Tier};
use zayden_app::modules::ModuleStates;
use zayden_app::state::AppState;
pub use zayden_core::OverlapError;
use zayden_core::ctx::{AutocompleteCtx, ComponentCtx, InvocationCtx, ModalCtx};
use zayden_core::error::HandlerError;
use zayden_core::module::{
    ModuleAutocomplete,
    ModuleCommand,
    ModuleComponent,
    ModuleModal,
};
pub use zayden_core::registry::RegistryBuilder;
use zayden_core::scope::CommandScope;
use zayden_core::{DispatchMap, Identity};

const MODULE_OFF: &str = "This module is turned off for this server.";

pub struct CommandRegistry {
    commands: HashMap<Cow<'static, str>, Arc<dyn ModuleCommand>>,
    components: DispatchMap<dyn ModuleComponent>,
    modals: DispatchMap<dyn ModuleModal>,
    autocompletes: HashMap<Cow<'static, str>, Arc<dyn ModuleAutocomplete>>,
}

impl CommandRegistry {
    #[must_use]
    pub fn from_builder(builder: RegistryBuilder) -> Arc<Self> {
        let parts = builder.into_parts();
        Arc::new(Self {
            commands: parts.commands,
            components: parts.components,
            modals: parts.modals,
            autocompletes: parts.autocompletes,
        })
    }

    #[must_use]
    pub fn definitions_for(
        &self,
        guild_id: GuildId,
        states: &ModuleStates,
    ) -> Vec<CreateCommand<'static>> {
        self.commands
            .values()
            .filter(|cmd| match cmd.scope() {
                CommandScope::Global => true,
                CommandScope::Guilds(ids) => ids.contains(&guild_id),
                CommandScope::ExcludeGuilds(ids) => !ids.contains(&guild_id),
            })
            .filter(|cmd| {
                cmd.module().is_none_or(|module| states.get(module) != Some(&false))
            })
            .map(|cmd| cmd.definition())
            .collect()
    }

    #[must_use]
    pub fn commands_by_module(
        &self,
    ) -> BTreeMap<&'static str, Vec<Cow<'static, str>>> {
        let mut map: BTreeMap<&'static str, Vec<Cow<'static, str>>> =
            BTreeMap::new();

        for cmd in self.commands.values() {
            if let Some(module) = cmd.module() {
                map.entry(module).or_default().push(cmd.name());
            }
        }

        map
    }

    pub async fn run_command(
        &self,
        ctx: &Context,
        interaction: &CommandInteraction,
        app: Arc<AppState>,
        me: Identity,
    ) -> Option<Result<(), HandlerError>> {
        let cmd = Arc::clone(self.commands.get(interaction.data.name.as_str())?);
        let span = info_span!(
            "command",
            module = cmd.module().unwrap_or("core"),
            command = &*cmd.name(),
            outcome = Empty,
        );

        if let (Some(module), Some(guild_id)) = (cmd.module(), interaction.guild_id)
            && module_disabled(&app, guild_id, module).await
        {
            reply_ephemeral(ctx, interaction, MODULE_OFF.to_string()).await;
            span.record("outcome", "disabled");
            return Some(Ok(()));
        }

        let required = cmd.metadata().required_tier;
        if required != Tier::Free {
            let scope = interaction.guild_id.map_or_else(
                || EntitlementScope::User(interaction.user.id.get()),
                |gid| {
                    EntitlementScope::UserInGuild(
                        interaction.user.id.get(),
                        gid.get(),
                    )
                },
            );
            if !app.entitlements.allows(scope, required).await {
                reply_ephemeral(ctx, interaction, upgrade_message(&app)).await;
                span.record("outcome", "upgrade_required");
                return Some(Ok(()));
            }
        }

        let cx = InvocationCtx { ctx, interaction, app, me };
        let result = cmd.run(&cx).instrument(span.clone()).await;
        span.record("outcome", outcome(&result));
        Some(result)
    }

    pub async fn run_component(
        &self,
        ctx: &Context,
        interaction: &ComponentInteraction,
        app: Arc<AppState>,
        me: Identity,
    ) -> Option<Result<(), HandlerError>> {
        let comp = Arc::clone(self.components.lookup(&interaction.data.custom_id)?);
        let span = info_span!(
            "component",
            module = comp.module().unwrap_or("core"),
            outcome = Empty,
        );

        if let (Some(module), Some(guild_id)) = (comp.module(), interaction.guild_id)
            && module_disabled(&app, guild_id, module).await
        {
            let strip = !comp.keep_when_disabled()
                && !interaction.message.flags.is_some_and(|flags| {
                    flags.contains(MessageFlags::IS_COMPONENTS_V2)
                });

            if strip {
                let response = CreateInteractionResponse::UpdateMessage(
                    CreateInteractionResponseMessage::new().components(Vec::new()),
                );
                if let Err(e) =
                    interaction.create_response(&ctx.http, response).await
                {
                    warn!(?e, "failed to strip components of disabled module");
                }
                let followup = CreateInteractionResponseFollowup::new()
                    .ephemeral(true)
                    .content(MODULE_OFF);
                if let Err(e) =
                    interaction.create_followup(&ctx.http, followup).await
                {
                    warn!(?e, "failed to send disabled module notice");
                }
            } else {
                reply_component_ephemeral(ctx, interaction, MODULE_OFF.to_string())
                    .await;
            }
            span.record("outcome", "disabled");
            return Some(Ok(()));
        }

        let required = comp.required_tier();
        if required != Tier::Free {
            let scope = interaction.guild_id.map_or_else(
                || EntitlementScope::User(interaction.user.id.get()),
                |gid| {
                    EntitlementScope::UserInGuild(
                        interaction.user.id.get(),
                        gid.get(),
                    )
                },
            );
            if !app.entitlements.allows(scope, required).await {
                reply_component_ephemeral(ctx, interaction, upgrade_message(&app))
                    .await;
                span.record("outcome", "upgrade_required");
                return Some(Ok(()));
            }
        }

        let cx = ComponentCtx { ctx, interaction, app, me };
        let result = comp.run(&cx).instrument(span.clone()).await;
        span.record("outcome", outcome(&result));
        Some(result)
    }

    pub async fn run_modal(
        &self,
        ctx: &Context,
        interaction: &ModalInteraction,
        app: Arc<AppState>,
        me: Identity,
    ) -> Option<Result<(), HandlerError>> {
        let modal = Arc::clone(self.modals.lookup(&interaction.data.custom_id)?);
        let cx = ModalCtx { ctx, interaction, app, me };
        Some(modal.run(&cx).await)
    }

    pub async fn run_autocomplete(
        &self,
        ctx: &Context,
        interaction: &CommandInteraction,
        app: Arc<AppState>,
        me: Identity,
    ) -> Option<Result<(), HandlerError>> {
        let auto =
            Arc::clone(self.autocompletes.get(interaction.data.name.as_str())?);
        let cx = AutocompleteCtx { ctx, interaction, app, me };
        Some(auto.run(&cx).await)
    }
}

async fn module_disabled(app: &AppState, guild_id: GuildId, module: &str) -> bool {
    match app.modules.states(guild_id.get().cast_signed()).await {
        Ok(states) => states.get(module) == Some(&false),
        Err(e) => {
            warn!(?e, %guild_id, module, "failed to read module state; allowing command");
            false
        },
    }
}

const fn outcome<T, E>(result: &Result<T, E>) -> &'static str {
    if result.is_ok() { "ok" } else { "error" }
}

fn upgrade_message(app: &AppState) -> String {
    app.upgrade_url.as_deref().map_or_else(
        || "This command requires a premium subscription.".to_string(),
        |url| {
            format!("This command requires a premium subscription. Upgrade at {url}")
        },
    )
}

async fn reply_component_ephemeral(
    ctx: &Context,
    interaction: &ComponentInteraction,
    content: String,
) {
    let response = CreateInteractionResponse::Message(
        CreateInteractionResponseMessage::new().ephemeral(true).content(content),
    );

    if let Err(e) = interaction.create_response(&ctx.http, response).await {
        warn!(?e, "failed to send ephemeral component reply");
    }
}

async fn reply_ephemeral(
    ctx: &Context,
    interaction: &CommandInteraction,
    content: String,
) {
    let response = CreateInteractionResponse::Message(
        CreateInteractionResponseMessage::new().ephemeral(true).content(content),
    );

    if let Err(e) = interaction.create_response(&ctx.http, response).await {
        warn!(?e, "failed to send ephemeral command reply");
    }
}
