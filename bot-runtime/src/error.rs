use zayden_core::error::{HandlerError, Respond};
use zayden_core::{CoreError, ModuleError};

pub type Result<T> = std::result::Result<T, BotError>;

#[derive(Debug)]
pub struct ModuleErrors(pub Vec<ModuleError>);

impl std::fmt::Display for ModuleErrors {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for (idx, error) in self.0.iter().enumerate() {
            if idx > 0 {
                f.write_str("; ")?;
            }
            error.fmt(f)?;
        }
        Ok(())
    }
}

#[derive(Debug)]
pub enum BotError {
    ZaydenCore(CoreError),

    Config(zayden_app::AppError),
    Application(zayden_app::applications::ApplicationError),
    Keyring(zayden_app::applications::ApplicationError),
    Jiff(jiff::Error),
    Modules(ModuleErrors),
    DuplicateTaskKind(&'static str),
    Other(String),
}

impl std::fmt::Display for BotError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ZaydenCore(e) => e.fmt(f),

            Self::Config(e) => e.fmt(f),
            Self::Application(e) => e.fmt(f),
            Self::Keyring(e) => write!(f, "custom bot keyring: {e}"),
            Self::Jiff(e) => e.fmt(f),
            Self::Modules(e) => e.fmt(f),
            Self::DuplicateTaskKind(kind) => {
                write!(f, "task kind {kind} is registered by two modules")
            },
            Self::Other(msg) => write!(f, "{msg}"),
        }
    }
}

impl std::error::Error for BotError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Config(e) => Some(e),
            Self::Application(e) | Self::Keyring(e) => Some(e),
            Self::ZaydenCore(e) => Some(e),
            Self::Jiff(e) => Some(e),
            Self::Modules(_) | Self::DuplicateTaskKind(_) | Self::Other(_) => None,
        }
    }
}

impl Respond for BotError {
    fn user_message(&self) -> Option<std::borrow::Cow<'_, str>> {
        match self {
            Self::ZaydenCore(e) => e.user_message(),

            Self::Config(_)
            | Self::Application(_)
            | Self::Keyring(_)
            | Self::Jiff(_)
            | Self::Modules(_)
            | Self::DuplicateTaskKind(_)
            | Self::Other(_) => None,
        }
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

impl From<zayden_app::AppError> for BotError {
    fn from(e: zayden_app::AppError) -> Self {
        Self::Config(e)
    }
}

impl From<zayden_app::applications::ApplicationError> for BotError {
    fn from(e: zayden_app::applications::ApplicationError) -> Self {
        Self::Application(e)
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
