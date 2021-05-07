use std::{collections::BTreeMap, sync::Arc};

use actix_web::{middleware, web, App, HttpRequest, HttpResponse, HttpServer, Responder};
use clap::Clap;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::{postgres::PgArguments, PgPool, Postgres};

use crate::{
    ast::AuthSettings,
    binding::bindings_from_json,
    config::Config,
    engine::{Evaluator, Importer, UpfrontImporter, WatchingImporter},
    query::build_queries,
    row_type::{convert_row, RowType},
    util::error_printing::PrintableError,
};

use super::{Command, Opts};

/// run in server mode
#[derive(Clap, Clone)]
pub struct Server {
    /// directory use for server
    directory: String,

    #[clap(short, long, default_value = "2332")]
    port: usize,

    #[clap(short, long, default_value = "10")]
    max_connections: u32,

    #[clap(short, long, default_value = "sql")]
    extension: String,

    #[clap(short, long)]
    watch: bool,
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
    data: QueryStatus<A>,
}

#[derive(Serialize)]
#[serde(tag = "status")]
pub enum QueryStatus<A> {
    #[serde(rename = "success")]
    Success { data: A },
    #[serde(rename = "error")]
    Error { message: String },
}

// TODO allow COOKIE_NAME to change based on env vars
// TODO set env vars with lazy static
const COOKIE_NAME: &'static str = "justsql_token";

async fn root() -> impl Responder {
    format!("ok")
}

async fn auth_query<I: Importer>(
    req: HttpRequest,
    data: web::Json<Query>,
    evaluator: web::Data<Evaluator>,
    pool: web::Data<PgPool>,
    config: web::Data<Arc<Config>>,
) -> impl Responder {
    enum ReturnType {
        SetToken(String),
        RemoveToken,
        DoNothing,
    }

    let cookie = req.cookie(COOKIE_NAME);
    let pool = pool.get_ref();
    let data = data.into_inner();

    let (endpoint, payload) = (data.endpoint, data.payload);
    let return_type: anyhow::Result<ReturnType> =
        async {
            let bindings = bindings_from_json(payload)?;

            async {
                let mut tx = pool.begin().await?;
                let module = evaluator.endpoint(endpoint.as_str())?;
                let auth = module.auth.as_ref().ok_or_else(|| {
                    anyhow!("module at endpoint {} does not have any auth settings")
                })?;
                let auth_bindings = module.verify(
                    config.auth.as_ref(),
                    cookie.as_ref().map(|cookie| cookie.value()),
                )?;

                let statements = evaluator.evaluate_endpoint(
                    endpoint.as_str(),
                    &bindings,
                    auth_bindings.as_ref(),
                )?;
                let queries = build_queries(&statements)?;
                let mut query: Option<sqlx::query::Query<Postgres, PgArguments>> = None;

                for cur in queries {
                    if let Some(cur_query) = query {
                        cur_query.execute(&mut tx).await?;
                    }
                    query = Some(cur);
                }

                let query = query.ok_or_else(|| {
                    anyhow!("module at endpoint {} did not have any queries", endpoint)
                })?;

                let res: ReturnType =
                    match auth {
                        AuthSettings::RemoveToken => {
                            query.execute(&mut tx).await?;
                            ReturnType::RemoveToken
                        }

                        AuthSettings::VerifyToken(v) => {
                            let res = query.fetch_one(&mut tx).await?;
                            let data = convert_row(res)?;
                            let secret = config.auth.as_ref().ok_or_else(|| {
                                anyhow!("config does not have secrets configured")
                            })?;
                            match v.as_ref() {
                                None => ReturnType::DoNothing,
                                Some(exp) => {
                                    let data = secret.encode(&data, *exp)?;
                                    ReturnType::SetToken(data)
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
                            let secret = config.auth.as_ref().ok_or_else(|| {
                                anyhow!("config does not have secrets configured")
                            })?;
                            let data = secret.encode(&data, *exp)?;
                            ReturnType::SetToken(data)
                        }
                    };

                tx.commit().await?;
                Ok(res)
            }
            .await
        }
        .await;

    match return_type {
        Err(err) => HttpResponse::BadRequest().json(QueryResult::<()> {
            endpoint,
            data: QueryStatus::Error {
                message: err.to_string(),
            },
        }),
        Ok(value) => match (value, req.cookie(COOKIE_NAME)) {
            (ReturnType::RemoveToken, Some(c)) => {
                HttpResponse::Ok().del_cookie(&c).json(QueryResult {
                    endpoint,
                    data: QueryStatus::Success {
                        data: "Cookie is deleted.",
                    },
                })
            }
            (ReturnType::RemoveToken, None) => HttpResponse::BadRequest().json(QueryResult::<()> {
                endpoint,
                data: QueryStatus::Error {
                    message: "User was not logged in.".to_string(),
                },
            }),
            (ReturnType::DoNothing, _) => HttpResponse::Ok().json(QueryResult {
                endpoint,
                data: QueryStatus::Success {
                    data: "User is authorized.",
                },
            }),
            (ReturnType::SetToken(token), _) => {
                let cookie = config.cookie.build(COOKIE_NAME, token);
                HttpResponse::Ok().cookie(cookie).json(json!(QueryResult {
                    endpoint,
                    data: QueryStatus::Success {
                        data: "User is authorized. Cookie is set.",
                    },
                }))
            }
        },
    }
}

async fn run_queries<I: Importer>(
    req: HttpRequest,
    data: web::Json<Vec<Query>>,
    evaluator: web::Data<Evaluator>,
    pool: web::Data<PgPool>,
    config: web::Data<Arc<Config>>,
) -> impl Responder {
    let cookie = &req.cookie(COOKIE_NAME);
    let evaluator = evaluator.get_ref();
    let pool = pool.get_ref();
    let data = data.into_inner();
    let config_secret = &config.auth;

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
                let module = evaluator.endpoint(endpoint.as_str())?;
                let auth_bindings = module.verify(
                    config_secret.as_ref(),
                    cookie.as_ref().map(|cookie| cookie.value()),
                )?;

                let bindings = bindings_from_json(payload)?;

                async {
                    let mut tx = pool.begin().await?;
                    let statements = evaluator.evaluate_endpoint(
                        endpoint.as_str(),
                        &bindings,
                        auth_bindings.as_ref(),
                    )?;
                    let queries = build_queries(&statements)?;
                    let mut query: Option<sqlx::query::Query<Postgres, PgArguments>> = None;

                    for cur in queries {
                        if let Some(cur_query) = query {
                            cur_query.execute(&mut tx).await?;
                        }
                        query = Some(cur);
                    }

                    let query = query.ok_or_else(|| {
                        anyhow!("module at endpoint {} did not have any queries", endpoint)
                    })?;
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
                Ok(res) => QueryStatus::Success { data: res },
                Err(res) => QueryStatus::Error { message: res },
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

fn create_evaluator(directory: &str, extension: &str, watch: bool) -> anyhow::Result<Evaluator> {
    if watch {
        let importer = WatchingImporter::new(directory, extension)?;
        Ok(Evaluator::with_importer(importer))
    } else {
        let (importer, errors) = UpfrontImporter::new(directory, extension)?;
        if errors.len() != 0 {
            let mut buffer = String::new();
            let plural = if errors.len() > 1 { "s" } else { "" };
            eprint!("errors in the following file{}: \n", plural);
            for (file_name, error) in errors {
                error.print_error(&mut buffer, file_name.as_str())?;
                eprint!("{}\n", buffer);
                buffer.clear();
            }
            return Err(anyhow!("failed to import some sql files"));
        } else {
            Ok(Evaluator::with_importer(importer))
        }
    }
}

pub async fn run_server(cmd: Server) -> anyhow::Result<()> {
    // import all files
    let evaluator = create_evaluator(cmd.directory.as_str(), cmd.extension.as_str(), cmd.watch)?;

    let config = Config::read_config()?;
    let config = Arc::new(config);

    let pool = sqlx::postgres::PgPoolOptions::new()
        .connect(
            config
                .database
                .url
                .as_ref()
                .and_then(|v| v.value().map(|v| v.into_owned()))
                .ok_or_else(|| anyhow!("must have database url set in config"))?
                .as_str(),
        )
        .await?;

    for endpoint in evaluator.importer.get_all_endpoints()? {
        info!("using endpoint {}", endpoint)
    }

    HttpServer::new(move || {
        App::new()
            .wrap(middleware::Logger::default())
            .wrap(middleware::Compress::default())
            .data(config.clone())
            .data(pool.clone())
            .data(evaluator.clone())
            .route("/", web::get().to(root))
            .route(
                "/api/v1/auth",
                web::post().to(auth_query::<UpfrontImporter>),
            )
            .route(
                "/api/v1/query",
                web::post().to(run_queries::<UpfrontImporter>),
            )
    })
    .bind(format!("0.0.0.0:{}", cmd.port))?
    .run()
    .await?;

    Ok(())
}
