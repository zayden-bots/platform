use std::collections::{HashMap, HashSet};

use futures::future::join_all;
use serenity::all::{FullEvent, Message};
use tracing::error;
use zayden_core::ctx::{EventCtx, StartCtx};
use zayden_core::module::{EventKind, Module, Screen};
use zayden_core::{CronResult, ModuleError, OverlapError, RegistryBuilder};

use crate::BotError;
use crate::error::ModuleErrors;

pub struct ModuleSet {
    modules: Vec<Box<dyn Module>>,
    routes: HashMap<EventKind, Vec<usize>>,
}

impl ModuleSet {
    pub fn new(modules: Vec<Box<dyn Module>>) -> Result<Self, BotError> {
        let mut seen = HashSet::new();
        let mut routes: HashMap<EventKind, Vec<usize>> = HashMap::new();

        for (idx, module) in modules.iter().enumerate() {
            let id = module.id();
            if !seen.insert(id) {
                return Err(BotError::Other(format!("duplicate module id: {id}")));
            }
            for kind in module.events() {
                routes.entry(*kind).or_default().push(idx);
            }
        }

        Ok(Self { modules, routes })
    }

    pub fn register_all(
        &self,
        builder: &mut RegistryBuilder,
    ) -> Result<(), OverlapError> {
        for module in &self.modules {
            module.register(builder)?;
        }
        Ok(())
    }

    pub fn cron_jobs(&self) -> Vec<CronResult> {
        self.modules.iter().flat_map(|module| module.cron_jobs()).collect()
    }

    pub fn start(&self, cx: &StartCtx<'_>) {
        for module in &self.modules {
            module.on_start(cx);
        }
    }

    pub async fn screen_message(&self, cx: &EventCtx<'_>, msg: &Message) -> Screen {
        for module in self.routed(EventKind::Message) {
            match module.screen_message(cx, msg).await {
                Ok(Screen::Pass) => {},
                Ok(Screen::Consumed) => return Screen::Consumed,
                Err(e) => {
                    error!(
                        module = module.id(),
                        event = ?EventKind::Message,
                        error = ?e,
                        "message screen failed",
                    );
                    return Screen::Consumed;
                },
            }
        }
        Screen::Pass
    }

    pub async fn dispatch(&self, cx: &EventCtx<'_>, ev: &FullEvent) {
        let Some(kind) = EventKind::of(ev) else {
            return;
        };

        let results = join_all(self.routed(kind).map(|module| async move {
            (module.id(), module.on_event(cx, ev).await)
        }))
        .await;

        for (id, result) in results {
            if let Err(e) = result {
                error!(
                    module = id,
                    event = ?kind,
                    error = ?e,
                    "module event handler failed",
                );
            }
        }
    }

    fn routed(&self, kind: EventKind) -> impl Iterator<Item = &dyn Module> {
        self.routes
            .get(&kind)
            .into_iter()
            .flatten()
            .filter_map(|idx| self.modules.get(*idx).map(AsRef::as_ref))
    }
}

#[derive(Default)]
pub struct ModuleList {
    modules: Vec<Box<dyn Module>>,
    errors: Vec<ModuleError>,
}

impl ModuleList {
    pub fn add(&mut self, module: impl Module) -> &mut Self {
        self.modules.push(Box::new(module));
        self
    }

    pub fn try_add<M: Module>(
        &mut self,
        module: Result<M, ModuleError>,
    ) -> &mut Self {
        if let Some(module) = self.take(module) {
            self.add(module);
        }
        self
    }

    pub fn take<T>(&mut self, value: Result<T, ModuleError>) -> Option<T> {
        match value {
            Ok(value) => Some(value),
            Err(e) => {
                self.errors.push(e);
                None
            },
        }
    }

    pub fn finish(
        self,
        primary_jobs: Vec<CronResult>,
    ) -> Result<Assembly, BotError> {
        if self.errors.is_empty() {
            Ok(Assembly { modules: self.modules, primary_jobs })
        } else {
            Err(BotError::Modules(ModuleErrors(self.errors)))
        }
    }
}

pub struct Assembly {
    pub modules: Vec<Box<dyn Module>>,
    pub primary_jobs: Vec<CronResult>,
}
