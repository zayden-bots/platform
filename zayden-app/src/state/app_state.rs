use std::collections::HashMap;
use std::sync::Arc;

use secrecy::SecretString;
use sqlx::PgPool;
use tokio::sync::broadcast;
use tokio::sync::broadcast::Sender;

use crate::applications::{Keyring, Slot};
use crate::config::{BotConfig, RadioStation, SettingsRegistry};
use crate::entitlement::{EntitlementService, Tier};
use crate::events::AppEvent;
use crate::modules::ModuleStore;
use crate::services::http::ClientBuilderExt;
use crate::serving::ServingResolver;

fn http_client() -> reqwest::Client {
    reqwest::Client::builder().with_timeouts().build().unwrap_or_else(|e| {
        tracing::warn!(
            error = %e,
            "failed to build the shared HTTP client; falling back to the \
             default (no timeout)"
        );
        reqwest::Client::new()
    })
}

pub struct AppState {
    pub db: PgPool,
    pub settings: SettingsRegistry,
    pub entitlements: Arc<EntitlementService>,
    pub modules: Arc<ModuleStore>,
    pub serving: Arc<ServingResolver>,
    pub events: Sender<AppEvent>,
    pub http: reqwest::Client,
    pub discord_token: SecretString,
    pub ai_provider_key: SecretString,
    pub ai_api_endpoint: String,
    pub ai_model: String,
    pub ai_model_structured: String,
    pub ai_model_pro: String,
    pub google_api_key: SecretString,
    pub error_log_webhook: SecretString,
    pub normal_log_webhook: SecretString,
    /// Discord user/application ID of the bot itself.
    pub zayden_id: u64,
    pub maria_id: Option<u64>,
    pub zayden_guild: u64,
    pub auto_leave: bool,
    pub upgrade_url: Option<String>,
    pub keyring: Option<Arc<Keyring>>,

    pub sku_tiers: HashMap<u64, Tier>,
    pub radio_stations: Arc<[RadioStation]>,
}

impl AppState {
    #[must_use]
    pub fn new(pool: PgPool, config: &BotConfig) -> Self {
        let (events, _) = broadcast::channel(64);

        let settings = SettingsRegistry::new(pool.clone(), &events);

        let entitlements =
            Arc::new(EntitlementService::new(pool.clone(), events.clone()));
        EntitlementService::spawn_invalidator(&entitlements, events.subscribe());

        let modules = Arc::new(ModuleStore::new(pool.clone()));

        let serving = Arc::new(ServingResolver::new(pool.clone()));
        ServingResolver::spawn_invalidator(&serving, events.subscribe());

        let mut sku_tiers = HashMap::new();
        if let Some(sku) = config.discord_sku_pro {
            sku_tiers.insert(sku, Tier::Pro);
        }
        if let Some(sku) = config.discord_sku_ultra {
            sku_tiers.insert(sku, Tier::Ultra);
        }

        let keyring = match Keyring::from_env() {
            Ok(keyring) => Some(Arc::new(keyring)),
            Err(e) => {
                tracing::warn!(error = %e, "no keyring: delivery webhooks are disabled");
                None
            },
        };

        Self {
            db: pool,
            settings,
            entitlements,
            modules,
            serving,
            events,
            http: http_client(),
            discord_token: config.discord_token.clone(),
            ai_provider_key: config
                .ai_provider_key
                .clone()
                .unwrap_or_else(|| SecretString::from(String::new())),
            ai_api_endpoint: config.ai_api_endpoint.clone(),
            ai_model: config.ai_model.clone(),
            ai_model_structured: config.ai_model_structured.clone(),
            ai_model_pro: config.ai_model_pro.clone(),
            google_api_key: config
                .google_api_key
                .clone()
                .unwrap_or_else(|| SecretString::from(String::new())),
            error_log_webhook: config
                .error_log_webhook
                .clone()
                .unwrap_or_else(|| SecretString::from(String::new())),
            normal_log_webhook: config
                .normal_log_webhook
                .clone()
                .unwrap_or_else(|| SecretString::from(String::new())),
            zayden_id: config.zayden_id,
            maria_id: config.maria_id,
            zayden_guild: config.zayden_guild,
            auto_leave: config.auto_leave,
            upgrade_url: config.upgrade_url.clone(),
            keyring,
            sku_tiers,
            radio_stations: Arc::clone(&config.radio_stations),
        }
    }

    #[must_use]
    pub const fn default_application_id(&self, slot: Slot) -> Option<u64> {
        match slot {
            Slot::Zayden => Some(self.zayden_id),
            Slot::Maria => self.maria_id,
        }
    }

    #[must_use]
    pub fn subscribe(&self) -> broadcast::Receiver<AppEvent> {
        self.events.subscribe()
    }
}
