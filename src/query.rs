use std::collections::BTreeMap;

use sqlx::{postgres::PgArguments, PgPool, Postgres};

use crate::{
    binding::Binding,
    codegen::Module,
    row_type::{convert_row, RowType},
};

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

pub async fn run_query(
    module: &Module,
    pool: &PgPool,
    bindings: &BTreeMap<String, Binding>,
    auth_bindings: Option<&BTreeMap<String, Binding>>,
    // whether to rollback the query at the end
    rollback: bool,
) -> anyhow::Result<Vec<BTreeMap<String, RowType>>> {
    async {
        let mut tx = pool.begin().await?;
        let statements = module.evaluate(bindings, auth_bindings)?;
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
