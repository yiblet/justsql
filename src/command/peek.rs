use std::path::Path;

use super::{Command, Opts};
use crate::engine::{Importer, UpfrontImporter};
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
    fn run_command(&self, opt: &Opts) -> anyhow::Result<()> {
        let importer = UpfrontImporter::from_paths_or_print_error(&[self.module.as_ref()])
            .ok_or_else(|| anyhow!("importing sql failed"))?;

        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?
            .block_on(async {
                let config = crate::config::Config::read_config(opt.config.as_ref())
                    .context("config is needed to find postgres_url")?;

                let (bindings, auth_bindings) =
                    super::read_input(self.json.as_str(), self.auth.as_ref().map(String::as_str))?;

                let pool = crate::server::init::connect_to_db(&config, Some(1)).await?;

                let module = importer.get_module_from_location(
                    Path::new(self.module.as_str()).canonicalize()?.as_path(),
                )?;
                let res = crate::query::run_query(
                    module.as_ref(),
                    &importer,
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
