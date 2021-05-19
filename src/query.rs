use std::collections::BTreeMap;

use sqlx::{postgres::PgArguments, PgPool, Postgres};
use std::fmt::Write;

use crate::{
    binding::Binding,
    codegen::{Interp, Module, ParamType},
    engine::Importer,
    row_type::{convert_row, RowType},
};

/// maps params to bindings
pub fn evaluate<'a, I: Importer, A>(
    module: &Module,
    importer: &I,
    bindings: &'a BTreeMap<String, A>,
    auth_bindings: Option<&'a BTreeMap<String, A>>,
) -> anyhow::Result<Vec<(String, Vec<&'a A>)>> {
    module
        .sql
        .iter()
        .map(|stmt| {
            let (query, params) = build_query_statement(&module, importer, stmt.as_slice())?;
            let binding = bind_params(params.as_slice(), bindings, auth_bindings)?;
            Ok((query, binding))
        })
        .collect::<anyhow::Result<Vec<_>>>()
}

/// maps params to bindings
pub fn bind_params<'a, 'b, A>(
    params: &'b [ParamType],
    bindings: &'a BTreeMap<String, A>,
    auth_bindings: Option<&'a BTreeMap<String, A>>,
) -> anyhow::Result<Vec<&'a A>> {
    params
        .iter()
        .cloned()
        .map(|param| match param {
            ParamType::Param(param) => bindings
                .get(param.as_str())
                .ok_or_else(|| anyhow!("parameter {} does not exist", param)),
            ParamType::Auth(param) => auth_bindings
                .ok_or_else(|| anyhow!("must have auth token"))?
                .get(param.as_str())
                .ok_or_else(|| anyhow!("parameter {} does not exist", param)),
        })
        .collect::<anyhow::Result<_>>()
}

/// generates the postgres sql query
/// and the argument bindings in the exact right order
pub fn build_query_statement<'a, I: Importer>(
    module: &'a Module,
    importer: &'a I,
    statement: &'a [Interp],
) -> anyhow::Result<(String, Vec<ParamType>)> {
    let mut buf = String::new();
    let mut mapping = BTreeMap::new();
    let param_mapping = module
        .front_matter
        .params
        .iter()
        .map(|param| (param.as_str(), ParamType::Param(param.clone())))
        .collect();
    build_query_statement_helper(
        module,
        importer,
        &mut buf,
        &mut mapping,
        &param_mapping,
        statement.iter(),
    )?;

    let params = {
        // invert the btree
        let inv_mapping: BTreeMap<_, _> = mapping.into_iter().map(|tup| (tup.1, tup.0)).collect();

        // uses the fact that this is in sorted order and checks if the mappings
        // where number going from 1 to len(mapping) + 1
        if inv_mapping
            .keys()
            .zip(1..=(inv_mapping.len() + 1))
            .any(|(v1, v2)| *v1 != v2)
        {
            Err(anyhow!("not all variable bindings were set"))?
        } else {
            inv_mapping.into_iter().map(|entry| entry.1).collect()
        }
    };

    Ok((buf, params))
}

// recursive function for inlining all imports
fn build_query_statement_helper<'a, I, M>(
    module: &Module,
    importer: &'a M,
    writer: &mut String,
    mapping: &mut BTreeMap<ParamType, usize>,
    param_mapping: &BTreeMap<&str, ParamType>,
    statement: I,
) -> anyhow::Result<()>
where
    M: Importer,
    I: Iterator<Item = &'a Interp>,
{
    for interp in statement {
        match &interp {
            Interp::Literal(lit) => write!(writer, "{}", lit.as_str())?,
            Interp::AuthParam(param) => {
                let param = ParamType::Auth(param.clone());
                if !mapping.contains_key(&param) {
                    let cur = mapping.len() + 1;
                    mapping.insert(param.clone(), cur);
                }
                write!(writer, "${}", mapping[&param])?
            }
            Interp::Param(param) => {
                let param_type = param_mapping.get(param.as_str()).ok_or_else(|| {
                    anyhow!("could not map paramter {} to the right param type", param)
                })?;
                if !mapping.contains_key(param_type) {
                    let cur = mapping.len() + 1;
                    mapping.insert(param_type.clone(), cur);
                }
                write!(writer, "${}", mapping[param_type])?
            }

            Interp::CallSite(func, params) => {
                let imported_module = {
                    let (path, _) = module
                        .front_matter
                        .imports
                        .get(func)
                        .ok_or_else(|| anyhow!("could not find import for {}", func))?;

                    importer.get_module_from_location(path).map_err(|err| {
                        err.context(format!("could not import module for {}", func))
                    })?
                };

                let new_param_mapping: BTreeMap<&str, ParamType> = {
                    if params.len() != imported_module.front_matter.params.len() {
                        Err(anyhow!(
                            "number of parameters to do not match for imported module {}",
                            func
                        ))?
                    }

                    imported_module
                        .front_matter
                        .params
                        .iter()
                        .zip(params.iter())
                        .map(
                            |(new_param, old_param)| -> anyhow::Result<(&str, ParamType)> {
                                let param_type =
                                    param_mapping.get(old_param.as_str()).ok_or_else(|| {
                                        anyhow!(
                                            "could not map paramter {} to the right param type",
                                            old_param
                                        )
                                    })?;

                                Ok((new_param.as_str(), param_type.clone()))
                            },
                        )
                        .collect::<anyhow::Result<_>>()?
                };

                let new_statement = {
                    let first_statement = imported_module.sql.get(0).ok_or_else(|| {
                        anyhow!("imported module {} should have one statement", func)
                    })?;
                    first_statement.iter()
                };

                write!(writer, " ( /* start of import {} */\n", func)?;
                build_query_statement_helper(
                    imported_module.as_ref(),
                    importer,
                    writer,
                    mapping,
                    &new_param_mapping,
                    new_statement,
                )?;
                write!(writer, "\n) /* end of import {} */", func)?;
            }
        }
    }
    Ok(())
}

pub fn build_queries<'a>(
    statements: &'a Vec<(String, Vec<&Binding>)>,
) -> anyhow::Result<Vec<sqlx::query::Query<'a, Postgres, PgArguments>>> {
    let queries = statements
        .iter()
        .map(|(statement, bindings)| {
            let mut query = sqlx::query(statement);
            for binding in bindings {
                query = match *binding {
                    Binding::String(val) => query.bind(val),
                    Binding::Float(val) => query.bind(val),
                    Binding::Bool(val) => query.bind(val),
                    Binding::Int(val) => query.bind(val),
                    Binding::Json(val) => query.bind(val),
                    Binding::Null => {
                        let res: Option<String> = None;
                        query.bind(res)
                    }
                };
            }
            query
        })
        .collect();

    Ok(queries)
}

pub async fn run_query<I>(
    module: &Module,
    importer: &I,
    pool: &PgPool,
    bindings: &BTreeMap<String, Binding>,
    auth_bindings: Option<&BTreeMap<String, Binding>>,
    // whether to rollback the query at the end
    rollback: bool,
) -> anyhow::Result<Vec<BTreeMap<String, RowType>>>
where
    I: Importer,
{
    async {
        let mut tx = pool.begin().await?;
        let statements = evaluate(module, importer, bindings, auth_bindings)?;
        let queries = build_queries(&statements)?;
        let mut query: Option<sqlx::query::Query<Postgres, PgArguments>> = None;

        for cur in queries {
            if let Some(cur_query) = query {
                cur_query.execute(&mut tx).await?;
            }
            query = Some(cur);
        }

        let query = query.ok_or_else(|| anyhow!("module at endpoint did not have any queries"))?;
        let results = query
            .fetch_all(&mut tx)
            .await?
            .into_iter()
            .map(convert_row)
            .collect::<anyhow::Result<Vec<BTreeMap<String, RowType>>>>()?;
        if rollback {
            tx.rollback().await?;
        } else {
            tx.commit().await?;
        }
        Ok(results)
    }
    .await
}
