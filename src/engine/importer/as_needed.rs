use std::{
    path::Path,
    sync::{Arc, Mutex},
};

use crate::ast::Module;

use super::{importer::Importer, module_collection::ModuleCollection};

#[derive(Debug, Default)]
pub struct AsNeededImporter(Mutex<ModuleCollection>);

impl Importer for AsNeededImporter {
    fn get_module_from_endpoint<'a>(&'a self, endpoint: &str) -> anyhow::Result<Arc<Module>> {
        let collection = self
            .0
            .lock()
            .map_err(|_| anyhow!("ModuleCollection was poisened"))?;
        collection
            .endpoints
            .get(endpoint)
            .cloned()
            .ok_or_else(|| anyhow!("endpoint does not exist"))
    }

    fn get_module_from_location(&self, location: &Path) -> anyhow::Result<Arc<Module>> {
        let mut collection = self
            .0
            .lock()
            .map_err(|_| anyhow!("ModuleCollection was poisened"))?;

        match collection.locations.get(location).cloned() {
            Some(module) => Ok(module),
            None => {
                collection.insert(location.into())?;
                collection
                    .locations
                    .get(location)
                    .cloned()
                    .ok_or_else(|| anyhow!("failed to get location"))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use path_clean::PathClean;

    use super::*;

    #[test]
    fn path_test() {
        let path: PathBuf = ["../test/.."].iter().collect();
        let path = path.clean();
        assert_eq!(path.as_path(), Path::new("../"))
    }
}
