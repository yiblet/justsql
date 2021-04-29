use std::collections::BTreeMap;

use actix_web::{middleware, web, App, HttpResponse, HttpServer, Responder};
use anyhow::anyhow;
use clap::Clap;
use serde::{Deserialize, Serialize};
use serde_json::{Number, Value};
use sqlx::{postgres::PgArguments, PgPool, Postgres};

use crate::{
    binding::Binding,
    module::Module,
    query::build_query,
    read_module,
    row_type::{convert_row, RowType},
};

use super::{Command, Opts};

/// run in server mode
#[derive(Clap, Clone)]
pub struct Server {
    /// directory use for server
    glob: String,

    #[clap(short, long, default_value = "2332")]
    port: usize,

    #[clap(short, long, default_value = "10")]
    max_connections: u32,

    #[clap(short, long, default_value = "sql")]
    extension: String,
}

// TODO currently can only send over simplistic types
#[derive(Deserialize)]
pub struct Query {
    endpoint: String,
    payload: BTreeMap<String, Value>,
}

#[derive(Serialize)]
pub struct QueryResult<A> {
    #[serde(rename = "endpoint")]
    endpoint: String,
    #[serde(flatten)]
    data: QueryData<A>,
}

#[derive(Serialize)]
pub enum QueryData<A> {
    #[serde(rename = "data")]
    Data(A),
    #[serde(rename = "error")]
    Error(String),
}

async fn root() -> impl Responder {
    format!("ok")
}

async fn run_queries(
    data: web::Json<Vec<Query>>,
    modules: web::Data<BTreeMap<String, Module>>,
    pool: web::Data<PgPool>,
) -> impl Responder {
    let modules = modules.get_ref();
    let pool = pool.get_ref();
    let data = data.into_inner();

    let (endpoints, payloads) = data
        .into_iter()
        .map(|query| (query.endpoint, query.payload))
        .fold((vec![], vec![]), |(mut v1, mut v2), (e1, e2)| {
            v1.push(e1);
            v2.push(e2);
            (v1, v2)
        });

    let query_results =
        endpoints
            .iter()
            .zip(payloads.into_iter())
            .map(|(endpoint, payload)| async move {
                let module = modules
                    .get(endpoint.as_str())
                    .ok_or_else(|| anyhow!("endpoint does not exist"))?;

                let bindings: BTreeMap<String, Binding> = payload
                    .into_iter()
                    .map(|(val, res)| Ok((val, Binding::from_json(res)?)))
                    .collect::<anyhow::Result<BTreeMap<String, Binding>>>()?;

                async {
                    let mut tx = pool.begin().await?;
                    let (last, statements) = module.sql.split_last().ok_or_else(|| {
                        anyhow!(
                            "module at endpoint {} does not have any statements",
                            module.endpoint.as_ref().map_or("", String::as_str),
                        )
                    })?;
                    for statement in statements {
                        let query = build_query(statement.as_str(), &bindings, &module)?;
                        query.execute(&mut tx).await?;
                    }
                    let query = build_query(last.as_str(), &bindings, &module)?;
                    let results = query
                        .fetch_all(&mut tx)
                        .await?
                        .into_iter()
                        .map(convert_row)
                        .collect::<anyhow::Result<Vec<BTreeMap<String, RowType>>>>()?;
                    tx.commit().await?;
                    Ok(results)
                }
                .await
            });

    let results: Vec<anyhow::Result<Vec<BTreeMap<String, RowType>>>> =
        futures::future::join_all(query_results).await;

    let results: Vec<QueryResult<Vec<BTreeMap<String, RowType>>>> = results
        .into_iter()
        .zip(endpoints.into_iter())
        .map(|(res, endpoint)| QueryResult {
            endpoint,
            data: match res.map_err(|err| err.to_string()) {
                Ok(res) => QueryData::Data(res),
                Err(res) => QueryData::Error(res),
            },
        })
        .collect();

    HttpResponse::Ok().json(results)
}

impl Command for Server {
    fn run_command(&self, _opt: &Opts) -> anyhow::Result<()> {
        let clone = self.clone();
        actix_rt::System::new().block_on(run_server(clone))?;
        Ok(())
    }
}

pub async fn run_server(cmd: Server) -> anyhow::Result<()> {
    let uri = crate::util::get_var("POSTGRES_URL")?;

    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(cmd.max_connections)
        .connect(uri.as_str())
        .await?;

    let modules: BTreeMap<String, Module> = glob::glob(cmd.glob.as_str())?
        .filter_map(|file| {
            file.map_err(|err| err.into())
                .and_then(read_module)
                .map(|val| Some((val.endpoint.as_ref()?.clone(), val)))
                .transpose()
        })
        .collect::<anyhow::Result<BTreeMap<String, Module>>>()?;

    HttpServer::new(move || {
        App::new()
            .wrap(middleware::Logger::default())
            .wrap(middleware::Compress::default())
            .data(pool.clone())
            .data(modules.clone())
            .route("/", web::get().to(root))
            .route("/api/query", web::post().to(run_queries))
    })
    .bind(format!("0.0.0.0:{}", cmd.port))?
    .run()
    .await?;

    let res: anyhow::Result<_> = Ok(());
    res
}
