use std::{path::Path, sync::Arc};

use crate::ast::Module;
use anyhow::anyhow;
use path_clean::PathClean;

use super::{importer::Importer, module_collection::ModuleCollection};

#[derive(Debug, Default, Clone)]
pub struct UpfrontImporter(Arc<ModuleCollection>);

impl UpfrontImporter {
    pub fn from_glob(glob: &str) -> anyhow::Result<Self> {
        let mut modules = ModuleCollection::default();
        for file in glob::glob(glob)? {
            let file = file?;
            modules.insert(file)?;
        }

        Ok(Self(Arc::new(modules)))
    }
}

impl Importer for UpfrontImporter {
    fn get_module_from_endpoint(&self, endpoint: &str) -> anyhow::Result<Arc<Module>> {
        self.0
            .endpoints
            .get(endpoint)
            .cloned()
            .ok_or_else(|| anyhow!("module does not exist"))
    }

    fn get_module_from_location(&self, location: &Path) -> anyhow::Result<Arc<Module>> {
        self.0
            .locations
            .get(location)
            .cloned()
            .ok_or_else(|| anyhow!("module does not exist"))
    }
}
