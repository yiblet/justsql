use std::{fmt, path::Path, sync::Arc};

use crate::codegen::Module;

pub trait Importer: Send + Sync + 'static + fmt::Debug {
    fn get_module_from_endpoint(&self, endpoint: &str) -> anyhow::Result<Arc<Module>>;
    fn get_module_from_location(&self, location: &Path) -> anyhow::Result<Arc<Module>>;
    fn get_all_endpoints(&self) -> anyhow::Result<Vec<String>>;
}

impl Importer for Arc<dyn Importer> {
    fn get_module_from_endpoint(&self, endpoint: &str) -> anyhow::Result<Arc<Module>> {
        self.as_ref().get_module_from_endpoint(endpoint)
    }

    fn get_module_from_location(&self, location: &Path) -> anyhow::Result<Arc<Module>> {
        self.as_ref().get_module_from_location(location)
    }

    fn get_all_endpoints(&self) -> anyhow::Result<Vec<String>> {
        self.as_ref().get_all_endpoints()
    }
}
