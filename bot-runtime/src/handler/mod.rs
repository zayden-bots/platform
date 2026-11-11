use std::sync::{Arc, OnceLock};

use secrecy::ExposeSecret;
use serenity::all::{Event, EventHandler, FullEvent, RatelimitInfo};
use serenity::async_trait;
use serenity::model::prelude::Interaction;
use serenity::prelude::Context;
use tokio::sync::RwLock;
use tracing::{debug, error, trace, warn};
use zayden_app::state::AppState;
use zayden_core::ctx::EventCtx;
use zayden_core::module::Screen;
use zayden_core::{Identity, RuntimeState};

mod entitlement;
mod guild_create;
mod guild_delete;
mod guild_update;
mod interaction;
mod ready;

use crate::CommandRegistry;
use crate::delivery::{DeliveryHandle, DeliveryJob};
use crate::modules::ModuleSet;

pub struct Handler {
    pub app: Arc<AppState>,
    pub modules: Arc<ModuleSet>,
    pub bot_state: Arc<RwLock<RuntimeState>>,
    pub registry: Arc<CommandRegistry>,
    pub cron_started: OnceLock<()>,
    pub(crate) delivery: OnceLock<DeliveryHandle>,
    pub me: Identity,
}

#[async_trait]
impl EventHandler for Handler {
    fn filter_event(
        &self,
        _context: &Context,
        event: Box<Event>,
    ) -> Option<Box<Event>> {
        match &*event {
            Event::GuildCreate(_)
            | Event::GuildDelete(_)
            | Event::GuildUpdate(_)
            | Event::GuildMemberRemove(_)
            | Event::MessageCreate(_)
            | Event::ReactionAdd(_)
            | Event::ReactionRemove(_)
            | Event::PresenceUpdate(_)
            | Event::Ready(_)
            | Event::VoiceStateUpdate(_)
            | Event::InteractionCreate(_)
            | Event::ThreadCreate(_)
            | Event::ThreadDelete(_)
            | Event::EntitlementCreate(_)
            | Event::EntitlementUpdate(_)
            | Event::EntitlementDelete(_)
            | Event::WebhookUpdate(_) => Some(event),
            Event::CommandPermissionsUpdate(_)
            | Event::AutoModRuleCreate(_)
            | Event::AutoModRuleUpdate(_)
            | Event::AutoModRuleDelete(_)
            | Event::AutoModActionExecution(_)
            | Event::ChannelCreate(_)
            | Event::ChannelDelete(_)
            | Event::ChannelPinsUpdate(_)
            | Event::ChannelUpdate(_)
            | Event::GuildAuditLogEntryCreate(_)
            | Event::GuildBanAdd(_)
            | Event::GuildBanRemove(_)
            | Event::GuildEmojisUpdate(_)
            | Event::GuildIntegrationsUpdate(_)
            | Event::GuildMemberAdd(_)
            | Event::GuildMemberUpdate(_)
            | Event::GuildMembersChunk(_)
            | Event::GuildRoleCreate(_)
            | Event::GuildRoleDelete(_)
            | Event::GuildRoleUpdate(_)
            | Event::GuildStickersUpdate(_)
            | Event::InviteCreate(_)
            | Event::InviteDelete(_)
            | Event::MessageDelete(_)
            | Event::MessageDeleteBulk(_)
            | Event::MessageUpdate(_)
            | Event::ReactionRemoveAll(_)
            | Event::ReactionRemoveEmoji(_)
            | Event::Resumed(_)
            | Event::SoundboardSounds(_)
            | Event::SoundboardSoundCreate(_)
            | Event::SoundboardSoundUpdate(_)
            | Event::SoundboardSoundsUpdate(_)
            | Event::SoundboardSoundDelete(_)
            | Event::TypingStart(_)
            | Event::UserUpdate(_)
            | Event::VoiceServerUpdate(_)
            | Event::VoiceChannelStatusUpdate(_)
            | Event::IntegrationCreate(_)
            | Event::IntegrationUpdate(_)
            | Event::IntegrationDelete(_)
            | Event::StageInstanceCreate(_)
            | Event::StageInstanceUpdate(_)
            | Event::StageInstanceDelete(_)
            | Event::ThreadUpdate(_)
            | Event::ThreadListSync(_)
            | Event::ThreadMemberUpdate(_)
            | Event::ThreadMembersUpdate(_)
            | Event::GuildScheduledEventCreate(_)
            | Event::GuildScheduledEventUpdate(_)
            | Event::GuildScheduledEventDelete(_)
            | Event::GuildScheduledEventUserAdd(_)
            | Event::GuildScheduledEventUserRemove(_)
            | Event::MessagePollVoteAdd(_)
            | Event::MessagePollVoteRemove(_)
            | Event::ShardStageUpdate(_)
            | Event::ChannelInfo(_)
            | Event::VoiceChannelStartTimeUpdate(_)
            | Event::VoiceChannelEffectSend(_) => None,
            event => {
                error!(?event, "unhandled event type");
                None
            },
        }
    }

    #[expect(
        clippy::wildcard_enum_match_arm,
        reason = "future variants are caught in the filter function"
    )]
    async fn dispatch(&self, ctx: &Context, ev: &FullEvent) {
        let event_name: &'static str = ev.into();

        let ev_command_name = match ev {
            FullEvent::InteractionCreate {
                interaction: Interaction::Command(interaction),
                ..
            } => interaction.data.name.as_str(),
            _ => "",
        };

        let pool = self.app.db.clone();

        let cx = EventCtx { ctx, app: Arc::clone(&self.app), me: self.me };

        if let FullEvent::Message { new_message, .. } = ev
            && !new_message.author.bot()
            && self.modules.screen_message(&cx, new_message).await
                == Screen::Consumed
        {
            return;
        }

        let result = match ev {
            FullEvent::GuildCreate { guild, .. } => {
                if let Some(delivery) = self.delivery.get() {
                    delivery.request(DeliveryJob::Guild(guild.id));
                }
                Self::guild_create(self, ctx, guild, &pool).await
            },
            FullEvent::WebhookUpdate { guild_id, belongs_to_channel_id, .. } => {
                if let Some(delivery) = self.delivery.get() {
                    delivery.request(DeliveryJob::Webhooks(
                        *guild_id,
                        *belongs_to_channel_id,
                    ));
                }
                return;
            },
            FullEvent::GuildDelete { incomplete, .. } => {
                Self::guild_delete(self, incomplete, &pool).await
            },
            FullEvent::GuildUpdate { new_data, .. } => {
                Self::guild_update(new_data, &pool).await
            },

            FullEvent::Ready { data_about_bot, .. } => {
                Self::ready(self, ctx, data_about_bot).await
            },
            FullEvent::InteractionCreate { interaction, .. } => {
                let app = Arc::clone(&self.app);
                let registry = Arc::clone(&self.registry);
                Self::interaction_create(ctx, interaction, app, registry, self.me)
                    .await
            },

            FullEvent::EntitlementCreate { entitlement, .. } => {
                entitlement::entitlement_create(ctx, entitlement, &self.app).await
            },
            FullEvent::EntitlementUpdate { entitlement, .. } => {
                entitlement::entitlement_update(ctx, entitlement, &self.app).await
            },
            FullEvent::EntitlementDelete { entitlement, .. } => {
                entitlement::entitlement_delete(ctx, entitlement, &self.app).await
            },

            _ => Ok(()),
        };

        if let Err(e) = result {
            error!(
                error = ?e,
                event = event_name,
                command = ev_command_name,
                "error handling event",
            );
        }

        if !matches!(ev, FullEvent::InteractionCreate { .. }) {
            self.modules.dispatch(&cx, ev).await;
        }
    }

    async fn ratelimit(&self, data: RatelimitInfo) {
        Self::log_ratelimit(
            &data,
            self.app.error_log_webhook.expose_secret(),
            self.app.normal_log_webhook.expose_secret(),
        );
    }
}

impl Handler {
    fn log_ratelimit(
        data: &RatelimitInfo,
        error_webhook: &str,
        normal_webhook: &str,
    ) {
        fn extract_suffix(url: &str) -> Option<&str> {
            url.split_once("/webhooks/").map(|(_, suffix)| suffix)
        }

        let path = &data.path;
        let timeout_ms = u64::try_from(data.timeout.as_millis()).unwrap_or(u64::MAX);

        let path_suffix = extract_suffix(path);
        let is_webhook_match = |webhook_url: &str| -> bool {
            if webhook_url.is_empty() {
                return false;
            }
            path_suffix.is_some() && path_suffix == extract_suffix(webhook_url)
        };

        if data.path.ends_with("commands") {
            trace!(?data, "rate limited (commands)");
        } else if is_webhook_match(error_webhook) || is_webhook_match(normal_webhook)
        {
            debug!(
                path = %path,
                method = ?data.method,
                limit = data.limit,
                timeout_ms,
                "webhook rate limited",
            );
        } else {
            warn!(
                path = %path,
                method = ?data.method,
                limit = data.limit,
                timeout_ms,
                "rate limited",
            );
        }
    }
}
