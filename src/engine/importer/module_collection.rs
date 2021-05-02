use crate::ast::{Module, ModuleError};
use std::{collections::BTreeMap, path::PathBuf, sync::Arc};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ModuleCollectionError {
    #[error("endpoint {0} already in use")]
    AlreadyUsedEndpointError(String),
    #[error("{0}")]
    ModuleError(#[from] ModuleError),
}

#[derive(Default, Debug)]
pub struct ModuleCollection {
    pub endpoints: BTreeMap<String, Arc<Module>>,
    pub locations: BTreeMap<PathBuf, Arc<Module>>,
}

impl ModuleCollection {
    pub fn insert(&mut self, location: PathBuf) -> Result<(), ModuleCollectionError> {
        let module = Arc::new(Module::from_path(&location)?);

        // insert module endpoint
        if let Some(endpoint) = module.endpoint.as_ref() {
            if self.endpoints.contains_key(endpoint) {
                Err(ModuleCollectionError::AlreadyUsedEndpointError(
                    endpoint.to_owned(),
                ))?
            };
            self.endpoints.insert(endpoint.to_owned(), module.clone());
        }

        self.locations.insert(location, module);

        Ok(())
    }
}
