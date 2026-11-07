use std::collections::{HashMap, HashSet};

use futures::future::join_all;
use serenity::all::{FullEvent, Message};
use tracing::error;
use zayden_core::ctx::EventCtx;
use zayden_core::module::{EventKind, Module, Screen};
use zayden_core::{OverlapError, RegistryBuilder};

use crate::BotError;

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

    pub async fn screen_message(&self, cx: &EventCtx<'_>, msg: &Message) -> Screen {
        for module in self.routed(EventKind::Message) {
            match module.screen_message(cx, msg).await {
                Ok(Screen::Pass) => {},
                Ok(Screen::Consumed) => return Screen::Consumed,
                Err(e) => {
                    error!(module = module.id(), error = ?e, "message screen failed");
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
