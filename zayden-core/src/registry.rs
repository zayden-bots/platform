use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::Arc;

use crate::dispatch_map::{DispatchMap, OverlapError};
use crate::module::{
    ModuleAutocomplete,
    ModuleCommand,
    ModuleComponent,
    ModuleModal,
};

pub struct RegistryBuilder {
    commands: HashMap<Cow<'static, str>, Arc<dyn ModuleCommand>>,
    components: DispatchMap<dyn ModuleComponent>,
    modals: DispatchMap<dyn ModuleModal>,
    autocompletes: HashMap<Cow<'static, str>, Arc<dyn ModuleAutocomplete>>,
}

pub struct RegistryParts {
    pub commands: HashMap<Cow<'static, str>, Arc<dyn ModuleCommand>>,
    pub components: DispatchMap<dyn ModuleComponent>,
    pub modals: DispatchMap<dyn ModuleModal>,
    pub autocompletes: HashMap<Cow<'static, str>, Arc<dyn ModuleAutocomplete>>,
}

impl Default for RegistryBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl RegistryBuilder {
    #[must_use]
    pub fn new() -> Self {
        Self {
            commands: HashMap::new(),
            components: DispatchMap::new(),
            modals: DispatchMap::new(),
            autocompletes: HashMap::new(),
        }
    }

    pub fn add_command(&mut self, cmd: impl ModuleCommand + 'static) -> &mut Self {
        let name = cmd.name();
        self.commands.insert(name, Arc::new(cmd));
        self
    }

    pub fn add_component(
        &mut self,
        comp: impl ModuleComponent + 'static,
    ) -> Result<&mut Self, OverlapError> {
        let id_match = comp.id_match();
        self.components.insert(id_match, Arc::new(comp))?;
        Ok(self)
    }

    pub fn add_modal(
        &mut self,
        modal: impl ModuleModal + 'static,
    ) -> Result<&mut Self, OverlapError> {
        let id_match = modal.id_match();
        self.modals.insert(id_match, Arc::new(modal))?;
        Ok(self)
    }

    pub fn add_autocomplete(
        &mut self,
        auto: impl ModuleAutocomplete + 'static,
    ) -> &mut Self {
        let cmd = auto.command();
        self.autocompletes.insert(cmd, Arc::new(auto));
        self
    }

    #[must_use]
    pub fn into_parts(self) -> RegistryParts {
        RegistryParts {
            commands: self.commands,
            components: self.components,
            modals: self.modals,
            autocompletes: self.autocompletes,
        }
    }
}
