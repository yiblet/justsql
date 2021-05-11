use std::{io, path::Path, sync::Arc};

use crate::codegen::Module;

use super::{
    importer::Importer,
    module_collection::{ModuleCollection, ModuleCollectionError},
};

#[derive(Debug, Default)]
pub struct UpfrontImporter(ModuleCollection);

impl UpfrontImporter {
    pub fn new(
        directory: &str,
        extension: &str,
    ) -> io::Result<(Self, Vec<(String, ModuleCollectionError)>)> {
        let (collection, errors) = ModuleCollection::from_directory(directory, extension, false)?;
        Ok((Self(collection), errors))
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

    fn get_all_endpoints(&self) -> anyhow::Result<Vec<String>> {
        let res = self.0.endpoints.keys().cloned().collect();
        Ok(res)
    }
}
