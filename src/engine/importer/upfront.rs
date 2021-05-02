use std::{path::Path, sync::Arc};

use either::Either;

use crate::ast::Module;

use super::{
    importer::Importer,
    module_collection::{ModuleCollection, ModuleCollectionError},
};

#[derive(Debug, Default)]
pub struct UpfrontImporter(pub(crate) ModuleCollection);

impl UpfrontImporter {
    pub fn from_glob(
        glob: &str,
    ) -> anyhow::Result<Either<Self, Vec<(ModuleCollectionError, String)>>> {
        let mut modules = ModuleCollection::default();

        let mut errors = vec![];
        for file in glob::glob(glob)? {
            let file = file?;
            if let Err(err) = modules.insert(file.clone()) {
                let file_str = file.to_string_lossy().as_ref().to_owned();
                errors.push((err, file_str));
            }
        }

        if errors.len() != 0 {
            return Ok(Either::Right(errors));
        }

        Ok(Either::Left(Self(modules)))
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
