use std::collections::HashSet;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use secrecy::ExposeSecret;
use serenity::all::Http;
use tokio::signal::unix::{SignalKind, signal};
use tracing::{error, info, warn};
use zayden_app::applications::Keyring;
use zayden_app::config::BotConfig;
use zayden_app::events::listener::EventListener;
use zayden_app::state::AppState;
use zayden_app::telemetry::install_metrics;
use zayden_core::{
    CronCtx,
    ModuleError,
    WebhookPool,
    WorkerJob,
    WorkerJobResult,
    WorkerModule,
    WorkerModuleCtx,
};

use crate::cron::{EntitlementSweepCron, PlatformTaskGc};
use crate::error::ModuleErrors;
use crate::sqlx_lib::new_pool_with_retry;
use crate::{BotError, Result, logging, status};

mod cron;

pub async fn boot() -> Result<WorkerBoot> {
    if rustls::crypto::aws_lc_rs::default_provider().install_default().is_err() {
        warn!("Rustls CryptoProvider was already installed");
    }

    let _ = dotenvy::dotenv();
    let _ = dotenvy::from_path(PathBuf::from("bots").join("worker").join(".env"));

    let pool = new_pool_with_retry().await?;

    schema::migrator()
        .run(&pool)
        .await
        .map_err(|e| BotError::Other(format!("failed to run migrations: {e}")))?;
    info!("Database migrations applied");

    let config = BotConfig::load_without_token(&pool).await?;
    info!("BotConfig loaded successfully");

    let keyring = Keyring::from_env().map_err(BotError::Keyring)?;

    let app = Arc::new(AppState::new(pool.clone(), &config));
    EventListener::spawn(pool.clone(), app.events.clone());

    let webhooks = Arc::new(WebhookPool::new(pool, Arc::new(keyring)));

    Ok(WorkerBoot { app, config, webhooks })
}

pub struct WorkerBoot {
    pub app: Arc<AppState>,
    pub config: BotConfig,
    webhooks: Arc<WebhookPool>,
}

impl WorkerBoot {
    #[must_use]
    pub const fn module_ctx(&self) -> WorkerModuleCtx<'_> {
        WorkerModuleCtx { app: &self.app, config: &self.config }
    }

    pub async fn run(self, assembly: WorkerAssembly) -> Result<()> {
        let Self { app, config, webhooks } = self;

        logging(
            Arc::new(Http::without_token()),
            config.error_log_webhook.as_ref().map(ExposeSecret::expose_secret),
            config.normal_log_webhook.as_ref().map(ExposeSecret::expose_secret),
        )
        .await;
        status::record_worker_start(&app.db).await;
        status::spawn_worker_heartbeat(app.db.clone());
        install_metrics("worker", SocketAddr::from(([0, 0, 0, 0], 9100)));

        let cx = CronCtx { db: app.db.clone(), app: Arc::clone(&app), webhooks };
        for module in &assembly.modules {
            module.on_start(&cx);
        }

        let jobs = assembly
            .modules
            .iter()
            .flat_map(|module| module.jobs())
            .chain(assembly.jobs)
            .chain([
                EntitlementSweepCron::worker_job(Arc::clone(&app.entitlements)),
                PlatformTaskGc::worker_job(app.db.clone()),
            ])
            .filter_map(|job| match job {
                Ok(job) => Some(job),
                Err(e) => {
                    error!(error = ?e, "failed to create worker job");
                    None
                },
            })
            .collect::<Vec<WorkerJob>>();

        cron::run(jobs, cx, shutdown_signal()).await
    }
}

async fn shutdown_signal() {
    match signal(SignalKind::terminate()) {
        Ok(mut terminate) => {
            tokio::select! {
                _ = tokio::signal::ctrl_c() => {},
                _ = terminate.recv() => {},
            }
        },
        Err(e) => {
            warn!(error = ?e, "cannot install the SIGTERM handler; waiting on ctrl-c only");
            let _ = tokio::signal::ctrl_c().await;
        },
    }
    info!("worker shutting down");
}

#[derive(Default)]
pub struct WorkerList {
    modules: Vec<Box<dyn WorkerModule>>,
    jobs: Vec<WorkerJobResult>,
    errors: Vec<ModuleError>,
}

impl WorkerList {
    pub fn add(&mut self, module: impl WorkerModule) -> &mut Self {
        self.modules.push(Box::new(module));
        self
    }

    pub fn try_add<M: WorkerModule>(
        &mut self,
        module: std::result::Result<M, ModuleError>,
    ) -> &mut Self {
        if let Some(module) = self.take(module) {
            self.add(module);
        }
        self
    }

    pub fn take<T>(
        &mut self,
        value: std::result::Result<T, ModuleError>,
    ) -> Option<T> {
        match value {
            Ok(value) => Some(value),
            Err(e) => {
                self.errors.push(e);
                None
            },
        }
    }

    pub fn job(&mut self, job: WorkerJobResult) -> &mut Self {
        self.jobs.push(job);
        self
    }

    pub fn finish(self) -> std::result::Result<WorkerAssembly, BotError> {
        if !self.errors.is_empty() {
            return Err(BotError::Modules(ModuleErrors(self.errors)));
        }

        let mut seen = HashSet::new();
        let declared = self
            .modules
            .iter()
            .flat_map(|module| module.jobs())
            .flatten()
            .map(|job| job.id)
            .chain(self.jobs.iter().flatten().map(|job| job.id.clone()));
        for id in declared {
            if !seen.insert(id.clone()) {
                return Err(BotError::Other(format!("duplicate worker job {id}")));
            }
        }

        Ok(WorkerAssembly { modules: self.modules, jobs: self.jobs })
    }
}

pub struct WorkerAssembly {
    pub modules: Vec<Box<dyn WorkerModule>>,
    pub jobs: Vec<WorkerJobResult>,
}
