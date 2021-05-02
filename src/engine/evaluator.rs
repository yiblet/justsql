use std::{collections::BTreeMap, path::Path, sync::Arc};

use crate::ast::{Module, ParamType};

use super::importer::Importer;

#[derive(Debug, Clone)]
pub struct Evaluator {
    importer: Arc<dyn Importer>,
}

impl Evaluator {
    pub fn with_importer<I>(importer: I) -> Evaluator
    where
        I: Importer,
    {
        Self {
            importer: Arc::new(importer),
        }
    }

    pub fn endpoint(&self, endpoint: &str) -> anyhow::Result<Arc<Module>> {
        let module = self.importer.get_module_from_endpoint(endpoint)?;
        Ok(module)
    }

    pub fn evaluate_endpoint<'a, A>(
        &self,
        endpoint: &str,
        bindings: &'a BTreeMap<String, A>,
        auth_bindings: Option<&'a BTreeMap<String, A>>,
    ) -> anyhow::Result<Vec<(String, Vec<&'a A>)>> {
        let module = self.importer.get_module_from_endpoint(endpoint)?;
        self.evaluate(module.as_ref(), bindings, auth_bindings)
    }

    #[allow(dead_code)]
    pub fn evaluate_module<'a, A>(
        &self,
        location: &Path,
        bindings: &'a BTreeMap<String, A>,
        auth_bindings: Option<&'a BTreeMap<String, A>>,
    ) -> anyhow::Result<Vec<(String, Vec<&'a A>)>> {
        let module = self.importer.get_module_from_location(location)?;
        self.evaluate(module.as_ref(), bindings, auth_bindings)
    }

    fn evaluate<'a, A>(
        &self,
        module: &Module,
        bindings: &'a BTreeMap<String, A>,
        auth_bindings: Option<&'a BTreeMap<String, A>>,
    ) -> anyhow::Result<Vec<(String, Vec<&'a A>)>> {
        module
            .sql
            .iter()
            .map(|stmt| -> anyhow::Result<(String, Vec<&'a A>)> {
                let (res, mapping) = stmt.bind()?;
                let bindings: Vec<_> = mapping
                    .into_iter()
                    .map(|param| match param {
                        ParamType::Param(param) => bindings
                            .get(param)
                            .ok_or_else(|| anyhow!("parameter {} does not exist", param)),
                        ParamType::Auth(param) => auth_bindings
                            .ok_or_else(|| anyhow!("must be authorized"))?
                            .get(param)
                            .ok_or_else(|| anyhow!("parameter {} does not exist", param)),
                    })
                    .collect::<anyhow::Result<_>>()?;
                Ok((res, bindings))
            })
            .collect()
    }
}
