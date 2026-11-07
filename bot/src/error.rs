use zayden_core::CoreError;
use zayden_core::error::{HandlerError, Respond};

pub type Result<T> = std::result::Result<T, BotError>;

#[derive(Debug)]
pub enum BotError {
    Jellyfin(jellyfin::JellyfinError),
    Music(music::MusicError),

    ZaydenCore(CoreError),

    Config(zayden_app::AppError),
    CustomBot(zayden_app::custom_bots::CustomBotError),
    Jiff(jiff::Error),
    Other(String),
}

impl std::fmt::Display for BotError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ZaydenCore(e) => e.fmt(f),

            Self::Jellyfin(e) => e.fmt(f),
            Self::Music(e) => e.fmt(f),
            Self::Config(e) => e.fmt(f),
            Self::CustomBot(e) => e.fmt(f),
            Self::Jiff(e) => e.fmt(f),
            Self::Other(msg) => write!(f, "{msg}"),
        }
    }
}

impl std::error::Error for BotError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Jellyfin(e) => Some(e),
            Self::Music(e) => Some(e),
            Self::Config(e) => Some(e),
            Self::CustomBot(e) => Some(e),
            Self::ZaydenCore(e) => Some(e),
            Self::Jiff(e) => Some(e),
            Self::Other(_) => None,
        }
    }
}

impl Respond for BotError {
    fn user_message(&self) -> Option<std::borrow::Cow<'_, str>> {
        match self {
            Self::Jellyfin(e) => e.user_message(),
            Self::Music(e) => e.user_message(),

            Self::ZaydenCore(e) => e.user_message(),

            Self::Config(_)
            | Self::CustomBot(_)
            | Self::Jiff(_)
            | Self::Other(_) => None,
        }
    }
}

impl From<jellyfin::JellyfinError> for BotError {
    fn from(e: jellyfin::JellyfinError) -> Self {
        Self::Jellyfin(e)
    }
}

impl From<music::MusicError> for BotError {
    fn from(e: music::MusicError) -> Self {
        Self::Music(e)
    }
}

impl From<serenity::Error> for BotError {
    fn from(value: serenity::Error) -> Self {
        Self::ZaydenCore(CoreError::Serenity(value))
    }
}

impl From<sqlx::Error> for BotError {
    fn from(value: sqlx::Error) -> Self {
        Self::ZaydenCore(CoreError::Sqlx(value))
    }
}

impl From<jiff::Error> for BotError {
    fn from(e: jiff::Error) -> Self {
        Self::Jiff(e)
    }
}

impl From<bungie_api::BungieApiError> for BotError {
    fn from(e: bungie_api::BungieApiError) -> Self {
        Self::Other(e.to_string())
    }
}

impl From<deadlock::DeadlockError> for BotError {
    fn from(e: deadlock::DeadlockError) -> Self {
        Self::Other(e.to_string())
    }
}

impl From<zayden_app::AppError> for BotError {
    fn from(e: zayden_app::AppError) -> Self {
        Self::Config(e)
    }
}

impl From<zayden_app::custom_bots::CustomBotError> for BotError {
    fn from(e: zayden_app::custom_bots::CustomBotError) -> Self {
        Self::CustomBot(e)
    }
}

impl From<HandlerError> for BotError {
    fn from(e: HandlerError) -> Self {
        match e {
            HandlerError::Database(e) => Self::ZaydenCore(CoreError::Sqlx(e)),
            HandlerError::Discord(e) => Self::ZaydenCore(CoreError::Serenity(e)),
            HandlerError::Module { source, .. } => {
                Self::ZaydenCore(CoreError::Other(source.to_string()))
            },
        }
    }
}
