use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use jiff::Timestamp;
use secrecy::ExposeSecret;
use serde::{Deserialize, Serialize};
use serenity::all::{
    ApplicationId,
    ChannelId,
    HttpError,
    JsonErrorCode,
    MessageId,
    StatusCode,
    UserId,
    WebhookId,
};
use serenity::http::Http;
use sqlx::PgPool;
use tracing::warn;
use zayden_app::applications::Keyring;
use zayden_app::delivery::DeliveryStore;
use zayden_app::tasks::{NewTask, ScheduledTasks};

use crate::error::DeliveryError;
use crate::post::Post;
use crate::snowflake::as_i64;

pub const POST_TASK: &str = "platform.post";
pub const DM_TASK: &str = "platform.dm";

static QUEUE_SEQ: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PostPayload {
    pub channel_id: u64,
    pub edit_message_id: Option<u64>,
    pub message: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DmPayload {
    pub user_id: u64,
    pub message: serde_json::Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Delivered {
    Webhook(MessageId),
    Queued,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Edited {
    Done,
    Queued,
    Missing,
}

pub struct WebhookPool {
    db: PgPool,
    keyring: Arc<Keyring>,
    http: Http,
}

impl WebhookPool {
    #[must_use]
    pub fn new(db: PgPool, keyring: Arc<Keyring>) -> Self {
        Self { db, keyring, http: Http::without_token() }
    }

    pub async fn send(
        &self,
        channel_id: ChannelId,
        application_id: ApplicationId,
        post: &Post<'_>,
    ) -> Result<Delivered, DeliveryError> {
        let Some(hook) = DeliveryStore::hook(
            &self.db,
            as_i64(channel_id.get()),
            as_i64(application_id.get()),
        )
        .await?
        else {
            queue_post(&self.db, application_id, channel_id, None, post).await?;
            count("queued");
            return Ok(Delivered::Queued);
        };

        let token = self.keyring.open_webhook(hook.webhook_id, &hook.sealed)?;

        let mut message = post.to_webhook();
        if let Some(name) = hook.sender_name {
            message = message.username(name);
        }
        if let Some(url) = hook.sender_avatar_url {
            message = message.avatar_url(url);
        }

        let result = message
            .execute(
                &self.http,
                WebhookId::new(hook.webhook_id.cast_unsigned()),
                token.expose_secret(),
                true,
            )
            .await;

        match result {
            Ok(Some(m)) => {
                count("webhook");
                Ok(Delivered::Webhook(m.id))
            },
            Ok(None) => Err(DeliveryError::NoMessage),
            Err(e) if is_dead_webhook(&e) => {
                self.break_hook(channel_id, application_id, hook.webhook_id).await?;
                queue_post(&self.db, application_id, channel_id, None, post).await?;
                Ok(Delivered::Queued)
            },
            Err(e) => Err(e.into()),
        }
    }

    pub async fn edit(
        &self,
        channel_id: ChannelId,
        application_id: ApplicationId,
        message_id: MessageId,
        post: &Post<'_>,
    ) -> Result<Edited, DeliveryError> {
        let Some(hook) = DeliveryStore::hook(
            &self.db,
            as_i64(channel_id.get()),
            as_i64(application_id.get()),
        )
        .await?
        else {
            queue_post(&self.db, application_id, channel_id, Some(message_id), post)
                .await?;
            count("queued");
            return Ok(Edited::Queued);
        };

        let token = self.keyring.open_webhook(hook.webhook_id, &hook.sealed)?;

        let result = post
            .to_webhook_edit()
            .execute(
                &self.http,
                WebhookId::new(hook.webhook_id.cast_unsigned()),
                token.expose_secret(),
                message_id,
            )
            .await;

        match result {
            Ok(_) => {
                count("webhook");
                Ok(Edited::Done)
            },
            Err(e) if is_dead_webhook(&e) => {
                self.break_hook(channel_id, application_id, hook.webhook_id).await?;
                queue_post(
                    &self.db,
                    application_id,
                    channel_id,
                    Some(message_id),
                    post,
                )
                .await?;
                Ok(Edited::Queued)
            },
            Err(e) if is_missing_message(&e) => Ok(Edited::Missing),
            Err(e) => Err(e.into()),
        }
    }

    async fn break_hook(
        &self,
        channel_id: ChannelId,
        application_id: ApplicationId,
        webhook_id: i64,
    ) -> sqlx::Result<()> {
        DeliveryStore::mark_broken(
            &self.db,
            as_i64(channel_id.get()),
            as_i64(application_id.get()),
            webhook_id,
        )
        .await?;
        warn!(
            channel_id = channel_id.get(),
            application_id = application_id.get(),
            "delivery webhook is dead; falling back to the bot"
        );
        count("broken");
        Ok(())
    }
}

fn count(outcome: &'static str) {
    metrics::counter!("delivery_sends_total", "outcome" => outcome).increment(1);
}

fn is_dead_webhook(e: &serenity::Error) -> bool {
    let serenity::Error::Http(HttpError::UnsuccessfulRequest(r)) = e else {
        return false;
    };
    matches!(
        r.error.code,
        JsonErrorCode::UnknownWebhook | JsonErrorCode::InvalidWebhookToken
    ) || r.status_code == StatusCode::UNAUTHORIZED
}

fn is_missing_message(e: &serenity::Error) -> bool {
    let serenity::Error::Http(HttpError::UnsuccessfulRequest(r)) = e else {
        return false;
    };
    r.error.code == JsonErrorCode::UnknownMessage
        || r.status_code == StatusCode::FORBIDDEN
}

pub async fn queue_post(
    pool: &PgPool,
    application_id: ApplicationId,
    channel_id: ChannelId,
    edit_message_id: Option<MessageId>,
    post: &Post<'_>,
) -> Result<(), DeliveryError> {
    let now = Timestamp::now();
    let seq = QUEUE_SEQ.fetch_add(1, Ordering::Relaxed);
    let message = if edit_message_id.is_some() {
        serde_json::to_value(post.to_edit_message())?
    } else {
        serde_json::to_value(post.to_message())?
    };
    let payload = PostPayload {
        channel_id: channel_id.get(),
        edit_message_id: edit_message_id.map(MessageId::get),
        message,
    };

    let task = NewTask {
        kind: POST_TASK,
        key: format!("post:{channel_id}:{}:{seq}", now.as_nanosecond()),
        run_at: now,
        payload: serde_json::to_value(payload)?,
    };
    ScheduledTasks::ensure(pool, as_i64(application_id.get()), &[task]).await?;
    Ok(())
}

pub async fn queue_dm(
    pool: &PgPool,
    application_id: ApplicationId,
    user_id: UserId,
    key: String,
    run_at: Timestamp,
    post: &Post<'_>,
) -> Result<(), DeliveryError> {
    let payload = DmPayload {
        user_id: user_id.get(),
        message: serde_json::to_value(post.to_message())?,
    };

    let task = NewTask {
        kind: DM_TASK,
        key,
        run_at,
        payload: serde_json::to_value(payload)?,
    };
    ScheduledTasks::ensure(pool, as_i64(application_id.get()), &[task]).await?;
    Ok(())
}
