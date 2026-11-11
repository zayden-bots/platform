use std::borrow::Cow;

use serenity::all::{HttpError, JsonErrorCode, StatusCode};
use zayden_app::applications::ApplicationError;

pub trait Respond: std::error::Error {
    fn user_message(&self) -> Option<Cow<'_, str>> {
        None
    }
}

#[derive(Debug)]
pub enum HandlerError {
    Database(sqlx::Error),
    Discord(serenity::Error),
    Module {
        source: Box<dyn std::error::Error + Send + Sync>,
        user_message: Option<String>,
    },
}

impl HandlerError {
    pub fn new<E>(err: E) -> Self
    where
        E: std::error::Error + Send + Sync + 'static,
    {
        Self::Module { source: Box::new(err), user_message: None }
    }

    pub fn from_respond<E>(err: E) -> Self
    where
        E: Respond + Send + Sync + 'static,
    {
        let user_message = err.user_message().map(Cow::into_owned);
        Self::Module { source: Box::new(err), user_message }
    }

    #[must_use]
    pub fn user_message(&self) -> Option<&str> {
        match self {
            Self::Database(_) | Self::Discord(_) => None,
            Self::Module { user_message, .. } => user_message.as_deref(),
        }
    }

    #[must_use]
    pub fn inner(&self) -> &(dyn std::error::Error + Send + Sync) {
        match self {
            Self::Database(e) => e,
            Self::Discord(e) => e,
            Self::Module { source, .. } => source.as_ref(),
        }
    }
}

impl std::fmt::Display for HandlerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Database(e) => write!(f, "database error: {e}"),
            Self::Discord(e) => write!(f, "discord error: {e}"),
            Self::Module { source, .. } => source.fmt(f),
        }
    }
}

impl std::error::Error for HandlerError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Database(e) => Some(e),
            Self::Discord(e) => Some(e),
            Self::Module { source, .. } => Some(source.as_ref()),
        }
    }
}

impl From<sqlx::Error> for HandlerError {
    fn from(e: sqlx::Error) -> Self {
        Self::Database(e)
    }
}

impl From<serenity::Error> for HandlerError {
    fn from(e: serenity::Error) -> Self {
        Self::Discord(e)
    }
}

impl From<CoreError> for HandlerError {
    fn from(e: CoreError) -> Self {
        Self::from_respond(e)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ModuleError {
    #[error("module `{module}` requires `{key}`, which is not set")]
    MissingConfig { module: &'static str, key: &'static str },
    #[error("module `{module}` failed to initialise")]
    Init {
        module: &'static str,
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },
}

impl ModuleError {
    pub fn init(
        module: &'static str,
        source: impl Into<Box<dyn std::error::Error + Send + Sync>>,
    ) -> Self {
        Self::Init { module, source: source.into() }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum DeliveryError {
    #[error(transparent)]
    Sql(#[from] sqlx::Error),
    #[error(transparent)]
    Seal(#[from] ApplicationError),
    #[error("discord rejected the webhook call: {0}")]
    Discord(WebhookFailure),
    #[error(transparent)]
    Encode(#[from] serde_json::Error),
    #[error("the webhook did not return the posted message")]
    NoMessage,
}

#[derive(Debug, Clone, thiserror::Error)]
pub enum WebhookFailure {
    #[error("{status} {code:?}: {message}")]
    Response { status: u16, code: JsonErrorCode, message: String },
    #[error("request failed (timeout: {timeout}, connect: {connect})")]
    Request { timeout: bool, connect: bool },
    #[error("webhook call failed ({0})")]
    Other(&'static str),
}

impl From<serenity::Error> for WebhookFailure {
    fn from(e: serenity::Error) -> Self {
        match e {
            serenity::Error::Http(HttpError::UnsuccessfulRequest(r)) => {
                Self::Response {
                    status: r.status_code.as_u16(),
                    code: r.error.code,
                    message: r.error.message.to_string(),
                }
            },
            serenity::Error::Http(HttpError::Request(e)) => {
                Self::Request { timeout: e.is_timeout(), connect: e.is_connect() }
            },
            serenity::Error::Io(_) => Self::Other("io"),
            serenity::Error::Json(_) => Self::Other("json"),
            serenity::Error::Model(_) => Self::Other("model"),
            serenity::Error::Http(_) => Self::Other("http"),
            serenity::Error::Token(_) => Self::Other("token"),
            serenity::Error::Url(_) => Self::Other("url"),
            serenity::Error::Gateway(_) => Self::Other("gateway"),
            _ => Self::Other("other"),
        }
    }
}

#[derive(Debug)]
pub enum CoreError {
    MissingGuildId,
    NotInteractionAuthor,

    MessageConflict,
    MissingData(String),

    InvalidOption(String),

    Serenity(serenity::Error),
    // region: Sqlx
    Sqlx(sqlx::Error),
    Other(String),
    // endregion
}

impl CoreError {
    pub fn missing_data(s: impl Into<String>) -> Self {
        Self::MissingData(s.into())
    }

    pub fn invalid_option(s: impl Into<String>) -> Self {
        Self::InvalidOption(s.into())
    }
}

impl std::fmt::Display for CoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingGuildId => {
                write!(f, "This command can only be used within a server.")
            },
            Self::NotInteractionAuthor => {
                write!(f, "You are not the author of this interaction.")
            },
            Self::InvalidOption(name) => {
                write!(
                    f,
                    "Required option `{name}` is missing or has an unexpected type"
                )
            },
            Self::MessageConflict => write!(
                f,
                "Command is already awaiting interaction. Please respond to previous command first."
            ),
            Self::MissingData(ctx) => write!(f, "missing data: {ctx}"),

            Self::Serenity(serenity::Error::Http(
                HttpError::UnsuccessfulRequest(resp),
            )) if matches!(
                resp.status_code,
                StatusCode::INTERNAL_SERVER_ERROR | StatusCode::SERVICE_UNAVAILABLE
            ) =>
            {
                write!(
                    f,
                    "It looks like Discord is currently experiencing some server issues. Please try your request again shortly. If the problem persists, please contact OscarSix for more details."
                )
            },

            Self::Serenity(serenity::Error::Http(
                HttpError::UnsuccessfulRequest(resp),
            )) => match resp.error.code {
                JsonErrorCode::UnknownChannel => {
                    write!(f, "Channel already deleted")
                },
                JsonErrorCode::UnknownMessage => {
                    write!(f, "Message was unexpectably deleted. Please try again.")
                },
                JsonErrorCode::UnknownWebhook => write!(f, "Unknown Webhook"),
                JsonErrorCode::UnknownInteraction => write!(
                    f,
                    "An error occurred while processing the interaction. Please try again."
                ),
                JsonErrorCode::MissingAccess => write!(
                    f,
                    "I don't have access to that resource. Please check my role or permissions, or ask a server admin for help."
                ),
                JsonErrorCode::LackPermissionsForAction => write!(
                    f,
                    "I'm missing permissions perform that action. Please contact a server admin to resolve this."
                ),
                JsonErrorCode::OperationOnArchivedThread => {
                    write!(f, "This thread has already been closed and archived.")
                },
                _ => write!(f, "serenity: {self:?}"),
            },
            Self::Serenity(e) => write!(f, "serenity: {e:?}"),

            Self::Sqlx(sqlx::Error::PoolTimedOut) => write!(
                f,
                "An internal error occurred while accessing data. Please try again shortly."
            ),
            Self::Sqlx(sqlx::Error::ColumnDecode { index, source })
                if source.is::<sqlx::error::UnexpectedNullError>() =>
            {
                write!(
                    f,
                    "Unexpected null found at {index}, please contact OscarSix to resolve."
                )
            },
            Self::Sqlx(e) => write!(f, "sqlx: {e:?}"),
            Self::Other(msg) => write!(f, "{msg}"),
        }
    }
}

impl std::error::Error for CoreError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Serenity(e) => Some(e),
            Self::Sqlx(e) => Some(e),
            Self::MissingGuildId
            | Self::NotInteractionAuthor
            | Self::MessageConflict
            | Self::MissingData(_)
            | Self::InvalidOption(_)
            | Self::Other(_) => None,
        }
    }
}

impl Respond for CoreError {
    fn user_message(&self) -> Option<Cow<'_, str>> {
        match self {
            Self::MissingGuildId
            | Self::NotInteractionAuthor
            | Self::MessageConflict => Some(Cow::Owned(self.to_string())),

            Self::Serenity(serenity::Error::Http(
                HttpError::UnsuccessfulRequest(resp),
            )) if matches!(
                resp.status_code,
                StatusCode::INTERNAL_SERVER_ERROR | StatusCode::SERVICE_UNAVAILABLE
            ) =>
            {
                Some(Cow::Owned(self.to_string()))
            },

            Self::Serenity(serenity::Error::Http(
                HttpError::UnsuccessfulRequest(resp),
            )) => match resp.error.code {
                JsonErrorCode::UnknownChannel
                | JsonErrorCode::UnknownMessage
                | JsonErrorCode::UnknownWebhook
                | JsonErrorCode::UnknownInteraction
                | JsonErrorCode::MissingAccess
                | JsonErrorCode::LackPermissionsForAction
                | JsonErrorCode::OperationOnArchivedThread => {
                    Some(Cow::Owned(self.to_string()))
                },
                _ => None,
            },

            Self::Sqlx(sqlx::Error::PoolTimedOut) => {
                Some(Cow::Owned(self.to_string()))
            },
            Self::Sqlx(sqlx::Error::ColumnDecode { source, .. })
                if source.is::<sqlx::error::UnexpectedNullError>() =>
            {
                Some(Cow::Owned(self.to_string()))
            },

            Self::MissingData(_)
            | Self::InvalidOption(_)
            | Self::Other(_)
            | Self::Serenity(_)
            | Self::Sqlx(_) => None,
        }
    }
}
