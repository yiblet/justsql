use std::{fmt, path::Path, sync::Arc};

use crate::codegen::Module;

pub trait Importer: Send + Sync + 'static + fmt::Debug {
    fn get_module_from_endpoint(&self, endpoint: &str) -> anyhow::Result<Arc<Module>>;
    fn get_module_from_location(&self, location: &Path) -> anyhow::Result<Arc<Module>>;
    fn get_all_endpoints(&self) -> anyhow::Result<Vec<String>>;
}
