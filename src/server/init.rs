use std::time::Duration;

use sqlx::{Pool, Postgres};

use crate::config::Config;

/// connects the
pub async fn connect_to_db(
    config: &Config,
    max_connections: Option<u32>,
) -> anyhow::Result<Pool<Postgres>> {
    info!("connecting to the database");
    let database_url = config
        .database
        .url
        .as_ref()
        .and_then(|v| v.value())
        .ok_or_else(|| anyhow!("must have database url set in config"))?;
    let pool = sqlx::postgres::PgPoolOptions::new()
        .connect_timeout(Duration::from_secs_f32(10f32))
        .max_connections(max_connections.unwrap_or(10u32))
        .connect(database_url.as_str())
        .await?;
    pool.acquire()
        .await
        .map_err(|_| anyhow!("failed to connect to database"))?;
    info!("succesfully connected to the database");
    Ok(pool)
}
