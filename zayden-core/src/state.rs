use std::sync::Arc;

use dashmap::DashMap;
use serenity::all::{Guild, GuildId, UserId};

use crate::cache::EmojiCache;
use crate::cron::CronJob;

/// The client's shared data: modules reach it with
/// `ctx.data::<RwLock<RuntimeState>>()`.
#[derive(Default)]
pub struct RuntimeState {
    emoji_cache: Arc<EmojiCache>,
    cron_jobs: Vec<CronJob>,
    guild_members: DashMap<GuildId, Vec<UserId>>,
}

impl RuntimeState {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn emojis(&self) -> Arc<EmojiCache> {
        Arc::clone(&self.emoji_cache)
    }

    pub fn emojis_mut(&mut self) -> &mut EmojiCache {
        Arc::make_mut(&mut self.emoji_cache)
    }

    pub fn set_emojis(&mut self, cache: EmojiCache) {
        self.emoji_cache = Arc::new(cache);
    }

    #[must_use]
    pub fn jobs(&self) -> &[CronJob] {
        &self.cron_jobs
    }

    pub const fn jobs_mut(&mut self) -> &mut Vec<CronJob> {
        &mut self.cron_jobs
    }

    #[must_use]
    pub const fn guild_members(&self) -> &DashMap<GuildId, Vec<UserId>> {
        &self.guild_members
    }

    pub fn guild_create(&self, guild: &Guild) {
        self.guild_members
            .insert(guild.id, guild.members.iter().map(|x| x.user.id).collect());
    }
}
