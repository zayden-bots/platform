use std::borrow::Cow;

use async_trait::async_trait;
use serenity::all::{CreateCommand, FullEvent, Message};
use zayden_app::entitlement::Tier;

use crate::cron::CronResult;
use crate::ctx::{
    AutocompleteCtx,
    ComponentCtx,
    EventCtx,
    InvocationCtx,
    ModalCtx,
    StartCtx,
};
use crate::dispatch_map::OverlapError;
use crate::error::{HandlerError, ModuleError};
use crate::registry::RegistryBuilder;
use crate::scope::{CommandMetadata, CommandScope, IdMatch};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EventKind {
    GuildCreate,
    GuildMemberRemoval,
    Message,
    ReactionAdd,
    ReactionRemove,
    PresenceUpdate,
    Ready,
    VoiceStateUpdate,
    ThreadCreate,
    ThreadDelete,
}

impl EventKind {
    #[must_use]
    #[expect(
        clippy::wildcard_enum_match_arm,
        reason = "FullEvent is non_exhaustive; unrouted kinds are None"
    )]
    pub const fn of(ev: &FullEvent) -> Option<Self> {
        match ev {
            FullEvent::GuildCreate { .. } => Some(Self::GuildCreate),
            FullEvent::GuildMemberRemoval { .. } => Some(Self::GuildMemberRemoval),
            FullEvent::Message { .. } => Some(Self::Message),
            FullEvent::ReactionAdd { .. } => Some(Self::ReactionAdd),
            FullEvent::ReactionRemove { .. } => Some(Self::ReactionRemove),
            FullEvent::PresenceUpdate { .. } => Some(Self::PresenceUpdate),
            FullEvent::Ready { .. } => Some(Self::Ready),
            FullEvent::VoiceStateUpdate { .. } => Some(Self::VoiceStateUpdate),
            FullEvent::ThreadCreate { .. } => Some(Self::ThreadCreate),
            FullEvent::ThreadDelete { .. } => Some(Self::ThreadDelete),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    Pass,
    Consumed,
}

#[async_trait]
pub trait Module: Send + Sync + 'static {
    fn id(&self) -> &'static str;

    fn register(&self, b: &mut RegistryBuilder) -> Result<(), OverlapError>;

    fn events(&self) -> &'static [EventKind] {
        &[]
    }

    /// Runs before every other message handler, for non-bot authors only.
    /// Returning `Consumed` stops all further handling of the message.
    async fn screen_message(
        &self,
        _cx: &EventCtx<'_>,
        _msg: &Message,
    ) -> Result<Screen, HandlerError> {
        Ok(Screen::Pass)
    }

    async fn on_event(
        &self,
        _cx: &EventCtx<'_>,
        _ev: &FullEvent,
    ) -> Result<(), HandlerError> {
        Ok(())
    }

    /// Collected once, on the first `Ready`, and only when this process is the
    /// primary application.
    fn cron_jobs(&self) -> Vec<CronResult> {
        Vec::new()
    }

    /// Runs once per process, on the first `Ready`.
    fn on_start(&self, _cx: &StartCtx<'_>) {}
}

pub fn require<T>(
    module: &'static str,
    key: &'static str,
    value: Option<T>,
) -> Result<T, ModuleError> {
    value.ok_or(ModuleError::MissingConfig { module, key })
}

#[async_trait]
pub trait ModuleCommand: Send + Sync {
    fn name(&self) -> Cow<'static, str> {
        let command = self.definition();
        let name = serde_json::to_value(command).map_or_else(
            |_| String::new(),
            |value| {
                value
                    .get("name")
                    .and_then(|v| v.as_str())
                    .map_or_default(String::from)
            },
        );

        Cow::Owned(name)
    }

    fn definition(&self) -> CreateCommand<'static>;

    fn module(&self) -> Option<&'static str> {
        None
    }

    fn scope(&self) -> CommandScope {
        CommandScope::Global
    }

    fn metadata(&self) -> CommandMetadata {
        CommandMetadata::default()
    }

    async fn run(&self, cx: &InvocationCtx<'_>) -> Result<(), HandlerError>;
}

#[async_trait]
pub trait ModuleComponent: Send + Sync {
    fn id_match(&self) -> IdMatch;

    fn module(&self) -> Option<&'static str> {
        None
    }

    fn keep_when_disabled(&self) -> bool {
        false
    }

    fn required_tier(&self) -> Tier {
        Tier::Free
    }

    async fn run(&self, cx: &ComponentCtx<'_>) -> Result<(), HandlerError>;
}

#[async_trait]
pub trait ModuleModal: Send + Sync {
    fn id_match(&self) -> IdMatch;
    async fn run(&self, cx: &ModalCtx<'_>) -> Result<(), HandlerError>;
}

#[async_trait]
pub trait ModuleAutocomplete: Send + Sync {
    fn command(&self) -> Cow<'static, str>;
    async fn run(&self, cx: &AutocompleteCtx<'_>) -> Result<(), HandlerError>;
}
