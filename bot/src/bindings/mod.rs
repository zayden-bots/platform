use std::sync::Arc;

use accounts::AccountsModule;
use ai::AiModule;
use async_trait::async_trait;
use deadlock::DeadlockModule;
use destiny2::{Destiny2Module, ParentToken};
use family::FamilyModule;
use gambling::{CoinReward, GamblingModule};
use gold_star::GoldStarModule;
use greetings::GreetingsModule;
use honeypot::HoneypotModule;
use hosting::HostingModule;
use jellyfin::JellyfinModule;
use levels::LevelsModule;
use lfg::LfgModule;
use llamad2::Llamad2Module;
use marathon::MarathonModule;
use misc::MiscModule;
use moderation::ModerationModule;
use music::MusicModule;
use palworld::PalworldModule;
use reaction_roles::ReactionRolesModule;
use serenity::all::GuildId;
use sqlx::PgPool;
use suggestions::SuggestionsModule;
use temp_voice::TempVoiceModule;
use ticket::TicketModule;
use verify::VerifyModule;
use watch::WatchModule;
use zayden_core::Module;
use zayden_core::error::HandlerError;

use crate::modules::ModuleSet;
use crate::registry::OverlapError;
use crate::{
    BotState,
    CommandRegistry,
    RegistryBuilder,
    ZAYDEN_TOKEN,
    zayden_token,
};

pub struct ZaydenParentToken;

#[async_trait]
impl ParentToken for ZaydenParentToken {
    async fn get(&self, db: &PgPool) -> Result<String, HandlerError> {
        ZAYDEN_TOKEN
            .get_or_try_init(|| zayden_token(db))
            .await
            .cloned()
            .map_err(HandlerError::Database)
    }
}

#[must_use]
pub fn modules(
    state: &BotState,
    parent_token: Arc<dyn ParentToken>,
    llamad2_guild: u64,
) -> Vec<Box<dyn Module>> {
    vec![
        Box::new(GreetingsModule::new()),
        Box::new(GoldStarModule::new()),
        Box::new(ModerationModule::new()),
        Box::new(HoneypotModule::new()),
        Box::new(MiscModule::new()),
        Box::new(ReactionRolesModule::new()),
        Box::new(SuggestionsModule::new()),
        Box::new(VerifyModule::new()),
        Box::new(FamilyModule::new()),
        Box::new(AccountsModule::new(Arc::clone(&state.dashboard_url))),
        Box::new(LevelsModule::new(Some(Arc::new(CoinReward)))),
        Box::new(GamblingModule::<BotState>::new()),
        Box::new(LfgModule::<BotState>::new()),
        Box::new(DeadlockModule::new(Arc::clone(&state.deadlock))),
        Box::new(MarathonModule::new(Arc::clone(&state.marathon))),
        Box::new(PalworldModule::new(Arc::clone(&state.palworld))),
        Box::new(Destiny2Module::<BotState>::new(
            Arc::clone(&state.bungie_client),
            parent_token,
        )),
        Box::new(Llamad2Module::<BotState>::new(GuildId::new(llamad2_guild))),
        Box::new(TempVoiceModule::new(Arc::clone(&state.voice_states))),
        Box::new(MusicModule::new(
            Arc::clone(&state.songbird),
            Arc::clone(&state.music),
            Arc::clone(&state.music_resolver),
        )),
        Box::new(TicketModule::new(Arc::clone(&state.wiki_index))),
        Box::new(AiModule::new()),
        Box::new(HostingModule::new(state.hosting.as_ref().map(Arc::clone))),
        Box::new(JellyfinModule::new(state.jellyfin.as_ref().map(Arc::clone))),
        Box::new(WatchModule::<BotState>::new(
            state.jellyfin.as_ref().map(Arc::clone),
        )),
    ]
}

pub fn build_registry(
    modules: &ModuleSet,
) -> Result<Arc<CommandRegistry>, OverlapError> {
    let mut builder = RegistryBuilder::new();
    modules.register_all(&mut builder)?;

    let registry = CommandRegistry::from_builder(builder);

    let by_module = registry.commands_by_module();
    if !by_module.is_empty() {
        tracing::info!(modules = ?by_module, "registered command modules");
    }

    Ok(registry)
}
