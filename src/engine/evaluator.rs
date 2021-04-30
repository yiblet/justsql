use std::{collections::BTreeMap, fmt, path::Path, sync::Arc};

use crate::ast::Module;
use anyhow::anyhow;

use super::importer::Importer;

#[derive(Debug, Clone)]
pub struct Evaluator<I: Importer> {
    importer: I,
}

impl<I: Importer + Send + Sync + 'static + Clone> Evaluator<I> {
    pub fn with_importer(importer: I) -> Evaluator<I>
    where
        I: Importer + Send + Sync + 'static,
    {
        Self { importer }
    }

    pub fn endpoint(&self, endpoint: &str) -> anyhow::Result<Arc<Module>> {
        let module = self.importer.get_module_from_endpoint(endpoint)?;
        Ok(module)
    }

    pub fn evaluate_endpoint<'a, A>(
        &self,
        endpoint: &str,
        bindings: &'a BTreeMap<String, A>,
    ) -> anyhow::Result<Vec<(String, Vec<&'a A>)>> {
        let module = self.importer.get_module_from_endpoint(endpoint)?;
        self.evaluate(module.as_ref(), bindings)
    }

    pub fn evaluate_module<'a, A>(
        &self,
        location: &Path,
        bindings: &'a BTreeMap<String, A>,
    ) -> anyhow::Result<Vec<(String, Vec<&'a A>)>> {
        let module = self.importer.get_module_from_location(location)?;
        self.evaluate(module.as_ref(), bindings)
    }

    fn evaluate<'a, A>(
        &self,
        module: &Module,
        bindings: &'a BTreeMap<String, A>,
    ) -> anyhow::Result<Vec<(String, Vec<&'a A>)>> {
        module
            .sql
            .iter()
            .map(|stmt| -> anyhow::Result<(String, Vec<&'a A>)> {
                let (res, mapping) = stmt.bind()?;
                let bindings: Vec<_> = mapping
                    .into_iter()
                    .map(|param| {
                        bindings
                            .get(param)
                            .ok_or_else(|| anyhow!("parameter {} does not exist", param))
                    })
                    .collect::<anyhow::Result<_>>()?;
                Ok((res, bindings))
            })
            .collect()
    }
}
