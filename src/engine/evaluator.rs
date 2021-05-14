use std::{collections::BTreeMap, sync::Arc};

use crate::{codegen::Module, query};

use super::importer::Importer;

#[derive(Debug, Clone)]
pub struct Evaluator {
    pub(crate) importer: Arc<dyn Importer>,
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

    pub fn evaluate_endpoint<'a, 'b: 'a, A>(
        &'b self,
        endpoint: &str,
        bindings: &'a BTreeMap<String, A>,
        auth_bindings: Option<&'a BTreeMap<String, A>>,
    ) -> anyhow::Result<Vec<(String, Vec<&'a A>)>> {
        let module = self.importer.get_module_from_endpoint(endpoint)?;
        query::evaluate(&module, &self.importer, bindings, auth_bindings)
    }
}
