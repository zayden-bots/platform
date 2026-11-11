use std::env::VarError;
use std::num::ParseIntError;
use std::string::FromUtf8Error;

use aws_lc_rs::error::Unspecified;
use thiserror::Error;

use super::Slot;

#[derive(Debug, Error)]
pub enum ApplicationError {
    #[error("{name} is not set")]
    MissingEnvVar {
        name: &'static str,
        #[source]
        source: VarError,
    },

    #[error("invalid custom bot key spec: {0}")]
    InvalidKeySpec(String),

    #[error("custom bot key id `{id}` is not a number")]
    KeyId {
        id: String,
        #[source]
        source: ParseIntError,
    },

    #[error("custom bot key {id} is not base64")]
    KeyNotBase64 {
        id: i16,
        #[source]
        source: base64::DecodeError,
    },

    #[error("custom bot key {id} must be 32 bytes")]
    KeyLength {
        id: i16,
        #[source]
        source: Unspecified,
    },

    #[error("no custom bot key with id {0} is loaded")]
    UnknownKey(i16),

    #[error("failed to encrypt the bot token")]
    Encrypt(#[source] Unspecified),

    #[error("the bot token failed to decrypt")]
    Decrypt(#[source] Unspecified),

    #[error("the stored nonce has the wrong length")]
    MalformedNonce(#[source] Unspecified),

    #[error("the decrypted bot token is not UTF-8")]
    NotUtf8(#[from] FromUtf8Error),

    #[error("no custom bot keyring is loaded")]
    NoKeyring,

    #[error("application {0} has no stored token")]
    NoToken(i64),

    #[error("unknown slot `{0}`")]
    UnknownSlot(String),

    #[error("unknown application kind `{0}`")]
    UnknownKind(String),

    #[error("the {} slot already has a default application", .0.name())]
    SlotTaken(Slot),

    #[error("application {0} is not a default application")]
    NotDefault(i64),

    #[error("application {0} is not a custom application")]
    NotCustom(i64),

    #[error("application {0} is not a registered custom application")]
    UnknownApplication(i64),

    #[error(
        "guild {guild_id} is already served in the {} slot by custom bot \
         {application_id}",
        slot.name()
    )]
    GuildTaken { guild_id: i64, slot: Slot, application_id: i64 },

    #[error(transparent)]
    Sqlx(#[from] sqlx::Error),
}
