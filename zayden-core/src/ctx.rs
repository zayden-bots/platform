use std::borrow::Cow;
use std::sync::Arc;

use serenity::all::{
    CommandInteraction,
    ComponentInteraction,
    Context,
    CreateInteractionResponse,
    CreateInteractionResponseMessage,
    ModalInteraction,
    Ready,
};
use zayden_app::config::BotConfig;
use zayden_app::state::AppState;

use crate::Identity;

pub struct ModuleCtx<'a> {
    pub app: &'a Arc<AppState>,
    pub config: &'a BotConfig,
    pub me: Identity,
}

impl ModuleCtx<'_> {
    #[must_use]
    pub const fn worker(&self) -> WorkerModuleCtx<'_> {
        WorkerModuleCtx { app: self.app, config: self.config }
    }
}

pub struct WorkerModuleCtx<'a> {
    pub app: &'a Arc<AppState>,
    pub config: &'a BotConfig,
}

pub struct StartCtx<'a> {
    pub ctx: &'a Context,
    pub app: Arc<AppState>,
    pub ready: &'a Ready,
    pub me: Identity,
}

pub struct TaskCtx<'a> {
    pub ctx: &'a Context,
    pub app: Arc<AppState>,
    pub me: Identity,
}

pub struct InvocationCtx<'a> {
    pub ctx: &'a Context,
    pub interaction: &'a CommandInteraction,
    pub app: Arc<AppState>,
    pub me: Identity,
}

pub struct EventCtx<'a> {
    pub ctx: &'a Context,
    pub app: Arc<AppState>,
    pub me: Identity,
}

pub struct ComponentCtx<'a> {
    pub ctx: &'a Context,
    pub interaction: &'a ComponentInteraction,
    pub app: Arc<AppState>,
    pub me: Identity,
}

impl ComponentCtx<'_> {
    pub async fn ephemeral(
        &self,
        content: impl Into<Cow<'_, str>>,
    ) -> serenity::Result<()> {
        self.interaction
            .create_response(
                &self.ctx.http,
                CreateInteractionResponse::Message(
                    CreateInteractionResponseMessage::new()
                        .content(content.into())
                        .ephemeral(true),
                ),
            )
            .await
    }
}

pub struct ModalCtx<'a> {
    pub ctx: &'a Context,
    pub interaction: &'a ModalInteraction,
    pub app: Arc<AppState>,
    pub me: Identity,
}

pub struct AutocompleteCtx<'a> {
    pub ctx: &'a Context,
    pub interaction: &'a CommandInteraction,
    pub app: Arc<AppState>,
    pub me: Identity,
}
