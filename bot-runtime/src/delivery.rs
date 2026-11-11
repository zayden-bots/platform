use std::sync::Arc;

use secrecy::SecretString;
use serenity::all::{ChannelId, CreateWebhook, GuildId, Http, WebhookId};
use serenity::http::StatusCode;
use tokio::sync::broadcast::error::RecvError;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};
use tracing::{debug, info, warn};
use zayden_app::applications::Keyring;
use zayden_app::delivery::{DeliveryStore, NewHook};
use zayden_app::events::AppEvent;
use zayden_app::state::AppState;
use zayden_core::{Identity, as_i64};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DeliveryJob {
    Guild(GuildId),
    Webhooks(GuildId, ChannelId),
    Sweep,
}

#[derive(Clone)]
pub(crate) struct DeliveryHandle(UnboundedSender<DeliveryJob>);

impl DeliveryHandle {
    pub(crate) fn request(&self, job: DeliveryJob) {
        if self.0.send(job).is_err() {
            warn!("delivery reconciler is not running");
        }
    }
}

pub(crate) fn spawn(
    http: Arc<Http>,
    app: Arc<AppState>,
    me: Identity,
) -> Option<DeliveryHandle> {
    let Some(keyring) = app.keyring.clone() else {
        warn!("no keyring: this bot will not create delivery webhooks");
        return None;
    };

    let (tx, rx) = unbounded_channel();
    let handle = DeliveryHandle(tx);
    handle.request(DeliveryJob::Sweep);

    spawn_forwarder(&app, me, handle.clone());

    let reconciler = Reconciler { http, app, me, keyring, handle: handle.clone() };
    tokio::spawn(reconciler.run(rx));

    Some(handle)
}

fn spawn_forwarder(app: &AppState, me: Identity, handle: DeliveryHandle) {
    let mut events = app.subscribe();
    tokio::spawn(async move {
        loop {
            match events.recv().await {
                Ok(
                    AppEvent::DeliveryChanged(guild)
                    | AppEvent::ServingChanged(guild),
                ) => handle.request(DeliveryJob::Guild(GuildId::new(guild))),
                Ok(AppEvent::WebhookBroken { guild_id, application_id, .. })
                    if application_id == me.application_id.get() =>
                {
                    handle.request(DeliveryJob::Guild(GuildId::new(guild_id)));
                },
                Ok(_) => {},
                Err(RecvError::Lagged(_)) => handle.request(DeliveryJob::Sweep),
                Err(RecvError::Closed) => break,
            }
        }
    });
}

struct Reconciler {
    http: Arc<Http>,
    app: Arc<AppState>,
    me: Identity,
    keyring: Arc<Keyring>,
    handle: DeliveryHandle,
}

impl Reconciler {
    async fn run(self, mut rx: UnboundedReceiver<DeliveryJob>) {
        while let Some(first) = rx.recv().await {
            let mut batch = vec![first];
            while let Ok(job) = rx.try_recv() {
                if !batch.contains(&job) {
                    batch.push(job);
                }
            }

            for job in batch {
                match job {
                    DeliveryJob::Sweep => self.sweep().await,
                    DeliveryJob::Guild(guild) => self.reconcile(guild).await,
                    DeliveryJob::Webhooks(guild, channel) => {
                        self.check_channel(guild, channel).await;
                    },
                }
            }
        }
    }

    async fn sweep(&self) {
        match DeliveryStore::wanted_guilds(&self.app.db, self.me.application_i64())
            .await
        {
            Ok(guilds) => {
                for guild in guilds {
                    self.handle.request(DeliveryJob::Guild(GuildId::new(
                        guild.cast_unsigned(),
                    )));
                }
            },
            Err(e) => {
                warn!(error = ?e, "failed to list guilds with delivery targets");
            },
        }
    }

    async fn reconcile(&self, guild: GuildId) {
        let guild_id = as_i64(guild.get());
        let application_id = self.me.application_i64();

        let wanted = match DeliveryStore::wanted(
            &self.app.db,
            guild_id,
            application_id,
        )
        .await
        {
            Ok(wanted) => wanted,
            Err(e) => {
                warn!(error = ?e, guild_id = %guild, "failed to read wanted channels");
                return;
            },
        };
        let have = match DeliveryStore::for_guild(
            &self.app.db,
            guild_id,
            application_id,
        )
        .await
        {
            Ok(have) => have,
            Err(e) => {
                warn!(error = ?e, guild_id = %guild, "failed to read stored webhooks");
                return;
            },
        };

        for channel in &wanted {
            if !have.iter().any(|hook| hook.channel_id == *channel && !hook.broken) {
                self.provision(guild, *channel).await;
            }
        }

        for hook in have.iter().filter(|hook| !wanted.contains(&hook.channel_id)) {
            self.remove(hook.channel_id, hook.webhook_id).await;
        }
    }

    async fn remove(&self, channel_id: i64, webhook_id: i64) {
        let webhook = WebhookId::new(webhook_id.cast_unsigned());
        match self
            .http
            .delete_webhook(webhook, Some("Broadcast delivery removed"))
            .await
        {
            Ok(()) => {},
            Err(serenity::Error::Http(e))
                if e.status_code() == Some(StatusCode::NOT_FOUND) => {},
            Err(e) => {
                warn!(error = ?e, channel_id, webhook_id, "failed to delete webhook");
            },
        }

        if let Err(e) = DeliveryStore::delete(
            &self.app.db,
            channel_id,
            self.me.application_i64(),
            webhook_id,
        )
        .await
        {
            warn!(error = ?e, channel_id, webhook_id, "failed to forget webhook");
        }
    }

    async fn provision(&self, guild: GuildId, channel_id: i64) {
        let channel = ChannelId::new(channel_id.cast_unsigned());

        let existing = match channel.webhooks(&self.http).await {
            Ok(webhooks) => webhooks,
            Err(serenity::Error::Http(e))
                if e.status_code() == Some(StatusCode::FORBIDDEN) =>
            {
                info!(
                    channel_id,
                    "missing Manage Webhooks; broadcasts go through the bot"
                );
                return;
            },
            Err(e) => {
                warn!(error = ?e, channel_id, "failed to list channel webhooks");
                return;
            },
        };

        let reusable = existing.into_iter().find(|webhook| {
            webhook.application_id == Some(self.me.application_id)
                && webhook.token.is_some()
        });
        let webhook = match reusable {
            Some(webhook) => webhook,
            None => {
                let builder = CreateWebhook::new(self.me.slot.name())
                    .audit_log_reason("Broadcast delivery");
                match channel.create_webhook(&self.http, builder).await {
                    Ok(webhook) => webhook,
                    Err(e) => {
                        warn!(error = ?e, channel_id, "failed to create webhook");
                        return;
                    },
                }
            },
        };

        let Some(token) = &webhook.token else {
            warn!(channel_id, "webhook has no token");
            return;
        };

        let webhook_id = as_i64(webhook.id.get());
        let token = SecretString::from(token.expose_secret());
        let sealed = match self.keyring.seal_webhook(webhook_id, &token) {
            Ok(sealed) => sealed,
            Err(e) => {
                warn!(error = ?e, channel_id, "failed to seal webhook token");
                return;
            },
        };

        let hook = NewHook {
            channel_id,
            application_id: self.me.application_i64(),
            guild_id: as_i64(guild.get()),
            webhook_id,
            sealed,
        };
        if let Err(e) = DeliveryStore::upsert(&self.app.db, &hook).await {
            warn!(error = ?e, channel_id, "failed to store webhook");
        }
    }

    async fn check_channel(&self, guild: GuildId, channel: ChannelId) {
        let channel_id = as_i64(channel.get());
        let application_id = self.me.application_i64();

        let stored = match DeliveryStore::channel_hook(
            &self.app.db,
            channel_id,
            application_id,
        )
        .await
        {
            Ok(Some(hook)) if !hook.broken => hook,
            Ok(_) => return,
            Err(e) => {
                warn!(error = ?e, guild_id = %guild, "failed to read stored webhook");
                return;
            },
        };

        let listed = match channel.webhooks(&self.http).await {
            Ok(listed) => listed,
            Err(e) => {
                debug!(error = ?e, channel_id, "failed to list channel webhooks");
                return;
            },
        };

        if listed.iter().any(|w| as_i64(w.id.get()) == stored.webhook_id) {
            return;
        }

        if let Err(e) = DeliveryStore::mark_broken(
            &self.app.db,
            channel_id,
            application_id,
            stored.webhook_id,
        )
        .await
        {
            warn!(error = ?e, channel_id, "failed to mark webhook broken");
        }
    }
}
