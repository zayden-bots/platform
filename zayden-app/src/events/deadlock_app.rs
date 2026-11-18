use serde::{Deserialize, Serialize};
use sqlx::PgExecutor;

use super::Channel;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DeadlockAppEvent {
    Patch { client_version: u32 },
    MatchReady { discord_id: i64, match_id: u64 },
    TokenRevoked { token_id: i64 },
    LinkChanged { discord_id: i64 },
}

impl DeadlockAppEvent {
    #[must_use]
    pub fn decode(payload: &str) -> Option<Self> {
        serde_json::from_str(payload).ok()
    }

    pub async fn notify(&self, executor: impl PgExecutor<'_>) -> sqlx::Result<()> {
        let payload = serde_json::to_string(self)
            .map_err(|e| sqlx::Error::Encode(Box::new(e)))?;
        Channel::DeadlockApp.notify(executor, &payload).await
    }
}
