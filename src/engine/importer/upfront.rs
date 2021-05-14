use std::{path::Path, sync::Arc};

use crate::{codegen::Module, util::error_printing::PrintableError};

use super::{
    importer::Importer,
    module_collection::{ModuleCollection, ModuleCollectionError},
};

#[derive(Debug, Default)]
pub struct UpfrontImporter(ModuleCollection);

impl UpfrontImporter {
    pub fn from_paths_or_print_error(paths: &[&Path]) -> Option<Self> {
        let paths_iter = paths.iter().map(|path| {
            path.canonicalize()
                .map_err(|err| ModuleCollectionError::IOError(path.to_path_buf(), err))
        });

        let mut path_bufs = vec![];
        let mut errors = vec![];
        for path in paths_iter {
            match path {
                Ok(path) => path_bufs.push(path),
                Err(err) => errors.push(err),
            }
        }

        let paths = path_bufs
            .iter()
            .map(|path| path.as_path())
            .collect::<Vec<_>>();
        match Self::from_paths(paths.as_slice()) {
            Ok(importer) => Some(importer),
            Err(err) => {
                let mut buf = String::new();
                err.as_slice().print_error(&mut buf).ok();
                eprintln!("{}", buf);
                None
            }
        }
    }

    pub fn from_paths(paths: &[&Path]) -> Result<Self, Vec<ModuleCollectionError>> {
        let (collection, errors) = ModuleCollection::from_paths(paths);
        if errors.len() != 0 {
            Err(errors)
        } else {
            debug!(
                "succesfully imported {} endpoints, and {} modules",
                collection.endpoints.len(),
                collection.locations.len()
            );
            Ok(Self(collection))
        }
    }

    pub fn new(directory: &str, extension: &str) -> Result<Self, Vec<ModuleCollectionError>> {
        let (collection, errors) = ModuleCollection::from_directory(directory, extension, false);
        if errors.len() != 0 {
            Err(errors)
        } else {
            info!(
                "succesfully imported {} endpoints, and {} modules",
                collection.endpoints.len(),
                collection.locations.len()
            );
            Ok(Self(collection))
        }
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
