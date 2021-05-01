use std::{path::Path, sync::Arc};

use crate::ast::Module;

use super::{importer::Importer, module_collection::ModuleCollection};

#[derive(Debug, Default, Clone)]
pub struct UpfrontImporter(pub(crate) Arc<ModuleCollection>);

impl UpfrontImporter {
    pub fn from_glob(glob: &str) -> anyhow::Result<Self> {
        let mut modules = ModuleCollection::default();

        let mut errors = vec![];
        for file in glob::glob(glob)? {
            let file = file?;
            if let Err(err) = modules.insert(file.clone()) {
                errors.push(anyhow!(
                    "error in {:?}\n{}",
                    file.as_path().as_os_str(),
                    err
                ))
            }
        }

        if errors.len() != 0 {
            let err: Vec<String> = errors.into_iter().map(|err| err.to_string()).collect();
            return Err(anyhow!("{}", err.join("\n")));
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
