use actix_web::{web, HttpRequest, HttpResponse, Responder};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::{postgres::PgArguments, PgPool, Postgres};
use std::{collections::BTreeMap, sync::Arc};

use crate::{
    ast::AuthSettings,
    binding::Binding,
    config::Config,
    engine::Evaluator,
    query::{self, build_queries},
    row_type::{convert_row, RowType},
};

// TODO currently can only send over simplistic types
#[derive(Deserialize)]
pub struct Query {
    endpoint: String,
    payload: BTreeMap<String, Binding>,
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

pub async fn auth_query(
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
    let return_type: anyhow::Result<ReturnType> = async {
        let mut tx = pool.begin().await?;
        let module = evaluator.endpoint(endpoint.as_str())?;
        let auth = module
            .auth
            .as_ref()
            .ok_or_else(|| anyhow!("module at endpoint {} does not have any auth settings"))?;
        let auth_bindings = module.verify(
            config.auth.as_ref(),
            cookie.as_ref().map(|cookie| cookie.value()),
        )?;

        let statements =
            evaluator.evaluate_endpoint(endpoint.as_str(), &payload, auth_bindings.as_ref())?;
        let queries = build_queries(&statements)?;
        let mut query: Option<sqlx::query::Query<Postgres, PgArguments>> = None;

        for cur in queries {
            if let Some(cur_query) = query {
                cur_query.execute(&mut tx).await?;
            }
            query = Some(cur);
        }

        let query = query
            .ok_or_else(|| anyhow!("module at endpoint {} did not have any queries", endpoint))?;

        let res: ReturnType = match auth {
            AuthSettings::RemoveToken => {
                query.execute(&mut tx).await?;
                ReturnType::RemoveToken
            }

            AuthSettings::VerifyToken(v) => {
                let res = query.fetch_one(&mut tx).await?;
                let data = convert_row(res)?;
                let secret = config
                    .auth
                    .as_ref()
                    .ok_or_else(|| anyhow!("config does not have secrets configured"))?;
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
                let secret = config
                    .auth
                    .as_ref()
                    .ok_or_else(|| anyhow!("config does not have secrets configured"))?;
                let data = secret.encode(&data, *exp)?;
                ReturnType::SetToken(data)
            }
        };

        tx.commit().await?;
        Ok(res)
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

pub async fn run_queries(
    req: HttpRequest,
    data: web::Json<Vec<Query>>,
    evaluator: web::Data<Evaluator>,
    pool: web::Data<PgPool>,
    config: web::Data<Arc<Config>>,
) -> impl Responder {
    let evaluator = evaluator.get_ref();
    let pool = pool.get_ref();
    let data = data.into_inner();
    let config_secret = &config.auth;
    let cookie = &req.cookie(COOKIE_NAME);
    let cookie = cookie.as_ref().map(|v| v.value());

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
                let auth_bindings = module.verify(config_secret.as_ref(), cookie)?;

                query::run_query(
                    module.as_ref(),
                    pool,
                    &payload,
                    auth_bindings.as_ref(),
                    false,
                )
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
