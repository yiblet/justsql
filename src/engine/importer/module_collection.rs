use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use crate::ast::Module;
use path_clean::PathClean;

#[derive(Default, Debug)]
pub struct ModuleCollection {
    pub endpoints: BTreeMap<String, Arc<Module>>,
    pub locations: BTreeMap<PathBuf, Arc<Module>>,
}

impl ModuleCollection {
    pub fn insert(&mut self, location: PathBuf) -> anyhow::Result<()> {
        let module = Arc::new(Module::from_path(&location)?);

        // insert module endpoint
        if let Some(endpoint) = module.endpoint.as_ref() {
            if self.endpoints.contains_key(endpoint) {
                Err(anyhow!(
                    "failed importing {} endpoint {} is already in use",
                    location.as_path().to_string_lossy(),
                    endpoint
                ))?
            };
            self.endpoints.insert(endpoint.to_owned(), module.clone());
        }

        self.locations.insert(location, module);

        Ok(())
    }
}
