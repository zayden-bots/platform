use serenity::all::{ApplicationId, UserId};
use zayden_app::applications::{AppKind, Slot};

use crate::snowflake::as_i64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Identity {
    pub application_id: ApplicationId,
    pub user_id: UserId,
    pub slot: Slot,
    pub kind: AppKind,
}

impl Identity {
    #[must_use]
    pub const fn application_i64(self) -> i64 {
        as_i64(self.application_id.get())
    }

    #[must_use]
    pub const fn is_primary(self) -> bool {
        matches!(self.kind, AppKind::Default) && matches!(self.slot, Slot::Zayden)
    }
}
