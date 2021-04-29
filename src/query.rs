use std::collections::BTreeMap;

use sqlx::{postgres::PgArguments, Postgres};

use crate::{binding::Binding, module::Module};

pub fn build_query<'a>(
    statement: &'a str,
    bindings: &'a BTreeMap<String, Binding>,
    module: &'a Module,
) -> anyhow::Result<sqlx::query::Query<'a, Postgres, PgArguments>> {
    let mut query = sqlx::query(statement);
    for binding in module.bindings(bindings) {
        query = match binding? {
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
    Ok(query)
}
