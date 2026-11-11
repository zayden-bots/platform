mod store;

use jiff::Timestamp;
use serde::de::DeserializeOwned;
pub use store::ScheduledTasks;

pub const MAX_ATTEMPTS: i16 = 5;
pub const STALE_AFTER: jiff::SignedDuration = jiff::SignedDuration::from_mins(10);
pub const PLATFORM_STALE_AFTER: jiff::SignedDuration =
    jiff::SignedDuration::from_hours(1);
pub const CLAIM_TIMEOUT: jiff::SignedDuration = jiff::SignedDuration::from_mins(5);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewTask {
    pub kind: &'static str,
    pub key: String,
    pub run_at: Timestamp,
    pub payload: serde_json::Value,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Task {
    pub id: i64,
    pub application_id: i64,
    pub kind: String,
    pub key: String,
    pub run_at: Timestamp,
    pub payload: serde_json::Value,
    pub claimed_at: Timestamp,
    pub attempts: i16,
}

impl Task {
    pub fn payload<T: DeserializeOwned>(&self) -> Result<T, serde_json::Error> {
        serde_json::from_value(self.payload.clone())
    }

    #[must_use]
    pub fn is_stale(&self, now: Timestamp) -> bool {
        now.duration_since(self.run_at) > STALE_AFTER
    }
}
