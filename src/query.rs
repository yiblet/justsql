use sqlx::{postgres::PgArguments, Postgres};

use crate::binding::Binding;

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
