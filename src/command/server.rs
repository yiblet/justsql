use std::collections::BTreeMap;

use actix_web::{
    cookie::Cookie, middleware, web, App, HttpRequest, HttpResponse, HttpServer, Responder,
};
use anyhow::anyhow;
use clap::Clap;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::PgPool;

use crate::{
    binding::bindings_from_json,
    module::{AuthSettings, Module},
    query::build_query,
    read_module,
    row_type::{convert_row, RowType},
    util::{get_cookie_domain, get_cookie_http_only, get_cookie_secure},
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

// TODO allow COOKIE_NAME to change based on env vars
// TODO set env vars with lazy static
const COOKIE_NAME: &'static str = "justsql_token";

async fn root() -> impl Responder {
    format!("ok")
}

async fn auth_query(
    req: HttpRequest,
    data: web::Json<Query>,
    modules: web::Data<BTreeMap<String, Module>>,
    pool: web::Data<PgPool>,
) -> impl Responder {
    enum ReturnType {
        SetToken(String, String),
        RemoveToken,
        DoNothing,
    }

    let cookie = req.cookie(COOKIE_NAME);
    let modules = modules.get_ref();
    let pool = pool.get_ref();
    let data = data.into_inner();

    let (endpoint, payload) = (data.endpoint, data.payload);
    let return_type: anyhow::Result<ReturnType> = async move {
        let module = modules
            .get(endpoint.as_str())
            .ok_or_else(|| anyhow!("endpoint does not exist"))?;

        module.verify(cookie.as_ref().map(|cookie| cookie.value()))?;

        let auth = module
            .auth
            .as_ref()
            .ok_or_else(|| anyhow!("endpoint {} is not an auth endpoint"))?;

        let bindings = bindings_from_json(payload)?;

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

            let res: ReturnType = match auth {
                AuthSettings::RemoveToken => {
                    query.execute(&mut tx).await?;
                    ReturnType::RemoveToken
                }

                AuthSettings::VerifyToken(v) => {
                    let res = query.fetch_one(&mut tx).await?;
                    let data = convert_row(res)?;
                    match v.as_ref() {
                        None => ReturnType::DoNothing,
                        Some(exp) => {
                            let data = crate::server::auth::encode(&data, *exp)?;
                            let cookie_domain = get_cookie_domain()?;
                            ReturnType::SetToken(cookie_domain, data)
                        }
                    }
                }

                AuthSettings::SetToken(exp) => {
                    // TODO if the user specifies more than one row
                    // explain that exactly one row is expcted

                    // TODO change errors to explain what happens
                    // depending on whether or not the server is run
                    // with debug mode
                    let res = query.fetch_one(&mut tx).await?;
                    let data = convert_row(res)?;
                    let data = crate::server::auth::encode(&data, *exp)?;
                    let cookie_domain = get_cookie_domain()?;
                    ReturnType::SetToken(cookie_domain, data)
                }
            };

            tx.commit().await?;
            Ok(res)
        }
        .await
    }
    .await;

    return_type.map_or_else(
        |err| HttpResponse::BadRequest().json(json!({"error": err.to_string()})),
        |value| match (value, req.cookie(COOKIE_NAME)) {
            (ReturnType::RemoveToken, Some(c)) => HttpResponse::Ok()
                .del_cookie(&c)
                .json(json!({"success": "Cookie is deleted."})),

            (ReturnType::RemoveToken, None) | (ReturnType::DoNothing, _) => {
                HttpResponse::Ok().json(json!({"success": "User is authorized."}))
            }

            (ReturnType::SetToken(domain, token), _) => HttpResponse::Ok()
                .cookie(
                    Cookie::build(COOKIE_NAME, token)
                        .domain(domain)
                        .path("/")
                        .secure(get_cookie_secure())
                        .http_only(get_cookie_http_only())
                        .finish(),
                )
                .json(json!({"success": "User is authorized. Cookie is set."})),
        },
    )
}

async fn run_queries(
    req: HttpRequest,
    data: web::Json<Vec<Query>>,
    modules: web::Data<BTreeMap<String, Module>>,
    pool: web::Data<PgPool>,
) -> impl Responder {
    let cookie = req.cookie(COOKIE_NAME);
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

    let cookie_content = cookie.as_ref();
    let query_results =
        endpoints
            .iter()
            .zip(payloads.into_iter())
            .map(|(endpoint, payload)| async move {
                let module = modules
                    .get(endpoint.as_str())
                    .ok_or_else(|| anyhow!("endpoint does not exist"))?;
                module.verify(cookie_content.map(|cookie| cookie.value()))?;

                let bindings = bindings_from_json(payload)?;

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
            .route("/api/v1/auth", web::post().to(auth_query))
            .route("/api/v1/query", web::post().to(run_queries))
    })
    .bind(format!("0.0.0.0:{}", cmd.port))?
    .run()
    .await?;

    let res: anyhow::Result<_> = Ok(());
    res
}
