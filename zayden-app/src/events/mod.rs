mod channel;
pub mod listener;

pub use channel::Channel;
use serde::{Deserialize, Serialize};

use crate::entitlement::EntitlementScope;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AppEvent {
    ConfigChanged(u64),
    ModulesChanged { guild_id: u64, application_id: u64 },
    EntitlementChanged(EntitlementScope),
    PatreonPost(String),
    YoutubeUpload(String),
    HostingPaid(i64),
    ServingChanged(u64),
    CustomBotsChanged(u64),
    LoadoutsChanged,
    TaskScheduled(u64),
    DeliveryChanged(u64),
    WebhookBroken { guild_id: u64, channel_id: u64, application_id: u64 },
    Resync,
}
