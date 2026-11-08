#![recursion_limit = "256"]

use std::fs::File;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use secrecy::ExposeSecret;
use serenity::all::{ClientBuilder, GatewayIntents, Http, Token};
use songbird::Songbird;
use sqlx::PgPool;
use tokio::sync::{OnceCell, RwLock};
use tracing::info;

pub mod cron;
mod error;
mod handler;
mod legacy_bot_tokens;
mod metrics_layer;
pub mod module_sync;
mod modules;
pub mod registry;
pub mod sqlx_lib;
pub mod state;
pub mod webhook_logger;

pub use error::{BotError, Result};
pub use handler::Handler;
pub use modules::{Assembly, ModuleList};
pub use registry::{CommandRegistry, OverlapError, RegistryBuilder};
use tracing::warn;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{Layer, Registry, filter, fmt};
use zayden_app::config::BotConfig;
use zayden_app::events::listener::EventListener;
use zayden_app::migrations;
use zayden_app::state::AppState;
use zayden_app::telemetry::{LogFormat, install_metrics};
use zayden_core::{ModuleCtx, RuntimeState};

use crate::modules::ModuleSet;
use crate::sqlx_lib::new_pool_with_retry;
use crate::webhook_logger::WebhookLogger;

pub static ZAYDEN_TOKEN: OnceCell<String> = OnceCell::const_new();

async fn zayden_token(pool: &PgPool) -> sqlx::Result<String> {
    sqlx::query_scalar!("SELECT token FROM bot_tokens WHERE name = 'zayden'")
        .fetch_one(pool)
        .await
}

// BUG: A guild with custom bots will likely have unintended behaviour with this
// current implimentation
pub async fn parent_token(pool: &PgPool) -> sqlx::Result<String> {
    ZAYDEN_TOKEN.get_or_try_init(|| zayden_token(pool)).await.cloned()
}

pub fn build_registry(
    modules: &ModuleSet,
) -> std::result::Result<Arc<CommandRegistry>, OverlapError> {
    let mut builder = RegistryBuilder::new();
    modules.register_all(&mut builder)?;

    let registry = CommandRegistry::from_builder(builder);

    let by_module = registry.commands_by_module();
    if !by_module.is_empty() {
        tracing::info!(modules = ?by_module, "registered command modules");
    }

    Ok(registry)
}

pub struct Boot {
    pub app: Arc<AppState>,
    pub config: BotConfig,
    name: &'static str,
}

pub async fn boot(name: &'static str) -> Result<Boot> {
    if rustls::crypto::aws_lc_rs::default_provider().install_default().is_err() {
        warn!("Rustls CryptoProvider was already installed");
    }

    let _ = dotenvy::dotenv();
    let _ = dotenvy::from_path(PathBuf::from("bots").join(name).join(".env"));

    let pool = new_pool_with_retry().await?;

    migrations::migrator()
        .run(&pool)
        .await
        .map_err(|e| BotError::Other(format!("failed to run migrations: {e}")))?;
    info!("Database migrations applied");

    let bot_config = BotConfig::load(&pool).await?;
    info!("BotConfig loaded successfully");

    let app_state = Arc::new(AppState::new(pool.clone(), &bot_config));
    info!("AppState constructed successfully");

    EventListener::spawn(pool.clone(), app_state.events.clone());
    legacy_bot_tokens::spawn(pool.clone(), bot_config.bot_owner);

    Ok(Boot { app: app_state, config: bot_config, name })
}

impl Boot {
    #[must_use]
    pub const fn module_ctx(&self) -> ModuleCtx<'_> {
        ModuleCtx { app: &self.app, config: &self.config }
    }

    pub async fn run(
        self,
        assembly: Assembly,
        voice_manager: Option<Arc<Songbird>>,
    ) -> Result<()> {
        let Self { app: app_state, config: bot_config, name } = self;

        let bot_state_inner = RuntimeState::new();
        let primary_jobs = cron::collect_jobs(assembly.primary_jobs);
        let modules = Arc::new(ModuleSet::new(assembly.modules)?);
        let bot_state = Arc::new(RwLock::new(bot_state_inner));

        let registry =
            build_registry(&modules).map_err(|e| BotError::Other(e.to_string()))?;
        zayden_app::modules::validate(&registry.commands_by_module())?;

        let mut builder = ClientBuilder::new(
            bot_config
                .discord_token
                .expose_secret()
                .parse::<Token>()
                .map_err(serenity::Error::Token)?,
            GatewayIntents::GUILDS
                | GatewayIntents::GUILD_MEMBERS
                | GatewayIntents::GUILD_MESSAGES
                | GatewayIntents::GUILD_MESSAGE_REACTIONS
                | GatewayIntents::GUILD_VOICE_STATES
                | GatewayIntents::MESSAGE_CONTENT
                | GatewayIntents::GUILD_PRESENCES,
        );
        if let Some(voice_manager) = voice_manager {
            builder = builder.voice_manager(voice_manager);
        }
        let mut client = builder
            .data(Arc::clone(&bot_state))
            .event_handler(Arc::new(Handler {
                app: Arc::clone(&app_state),
                modules,
                bot_state,
                registry,
                cron_started: OnceLock::new(),
                primary_jobs,
            }))
            .await?;

        logging(
            Arc::clone(&client.http),
            bot_config.error_log_webhook.as_ref().map(ExposeSecret::expose_secret),
            bot_config.normal_log_webhook.as_ref().map(ExposeSecret::expose_secret),
        )
        .await;
        install_metrics(name, SocketAddr::from(([0, 0, 0, 0], 9100)));

        let runners = Arc::clone(&client.shard_manager.runners);
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(30));
            loop {
                interval.tick().await;
                for entry in runners.iter() {
                    let latency = entry.value().info.read().latency;
                    if let Some(latency) = latency {
                        metrics::gauge!(
                            "bot_gateway_latency_seconds",
                            "shard" => entry.key().0.to_string(),
                        )
                        .set(latency.as_secs_f64());
                    }
                }
            }
        });

        client.start_autosharded().await?;

        Ok(())
    }
}

async fn logging(
    http: Arc<Http>,
    error_log_url: Option<&str>,
    normal_log_url: Option<&str>,
) {
    let debug_log = match File::create("log.txt") {
        Ok(log_file) => Some(
            fmt::layer()
                .with_writer(log_file)
                .with_filter(filter::LevelFilter::INFO),
        ),
        Err(e) => {
            eprintln!(
                "warning: failed to create log.txt, file logging disabled: {e}"
            );
            None
        },
    };

    let json = LogFormat::from_env() == LogFormat::Json;
    let text_log = (!json).then(|| {
        fmt::layer()
            .with_writer(std::io::stdout)
            .with_filter(filter::LevelFilter::INFO)
    });
    let json_log = json.then(|| {
        fmt::layer()
            .json()
            .with_writer(std::io::stdout)
            .with_filter(filter::LevelFilter::INFO)
    });

    let webhook_log = WebhookLogger::new(http, error_log_url, normal_log_url).await;

    Registry::default()
        .with(debug_log)
        .with(text_log)
        .with(json_log)
        .with(webhook_log)
        .with(metrics_layer::layer())
        .init();
}
