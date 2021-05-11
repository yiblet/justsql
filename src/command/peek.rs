use super::{Command, Opts};
use crate::codegen::Module;
use anyhow::Context;
use clap::Clap;

/// run a query without committing the changes
#[derive(Clap)]
pub struct Peek {
    /// location of the sql file
    module: String,

    /// the payload as a json string or path to a file containing the payload
    json: String,

    /// the auth claims as a json string or path to a file containing the auth claims
    #[clap(short, long)]
    auth: Option<String>,

    /// show only the first output
    #[clap(short, long)]
    first: bool,
}

impl Command for Peek {
    fn run_command(&self, _opt: &Opts) -> anyhow::Result<()> {
        let module = Module::from_path(&self.module).context("failed to find file")?;

        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?
            .block_on(async {
                let config = crate::config::Config::read_config()
                    .context("config is needed to find postgres_url")?;

                let (bindings, auth_bindings) =
                    super::read_input(self.json.as_str(), self.auth.as_ref().map(String::as_str))?;

                let pool = sqlx::postgres::PgPoolOptions::new()
                    .max_connections(1)
                    .connect(
                        config
                            .database
                            .url
                            .and_then(|v| Some(v.value()?.into_owned()))
                            .ok_or_else(|| anyhow!("must have database url set in config"))?
                            .as_str(),
                    )
                    .await?;

                let res = crate::query::run_query(
                    &module,
                    &pool,
                    &bindings,
                    auth_bindings.as_ref(),
                    true,
                )
                .await?;

                if self.first {
                    println!("{}", serde_json::to_string_pretty(&res[0])?);
                } else {
                    println!("{}", serde_json::to_string_pretty(&res)?);
                }
                Ok::<_, anyhow::Error>(())
            })?;

        Ok(())
    }
}
