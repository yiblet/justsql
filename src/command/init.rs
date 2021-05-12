use super::{Command, Opts};
use clap::Clap;
use std::path::Path;
use std::io::prelude::*;


/// initialize justsql for this project with a basic justsql.config.yaml file
#[derive(Clap)]
pub struct Init {
    /// DB address to connect to (default: postgres://postgres:postgres@localhost:5432/postgres)
    database_url: Option<String>,

    /// algorithm to use for auth. ex: HS256, RS256, PS384, etc. (default: HS256)
    auth_alg: Option<String>,
}

// TODO: randomly generate the secret key each time
static DEFAULT_CONFIG: &'static str  = r#"
# sets the database url
database:
  url:
    # any field can be changed to a "from_env" value to pull the information
    # from an environment variable that's either in a .env or passed in
    from_env: $DATABASE_URL
    # (optional) defualt value if the environment variable is not set
    default: {database_default} 

auth:
  # auth algorithm
  algorithm: {auth_alg}
  # randomly generated key for secret_key_base64
  # created from running "head -c 32 < /dev/random | base64"
  # for production we recommend using a secure random number generator
  # to generate the key
  secret_key_base64: 7phkIkcWtlxOovDKbCxj9aFriq6KLyN/8wrnDMzJ3WE=

cookie:
  secure: true
  http_only: true
"#;

impl Command for Init {
    fn run_command(&self, _opt: &Opts) -> anyhow::Result<()> {
        let config_out_path = Path::new("justsql.config.yaml");
        let mut config_file = std::fs::File::create(&config_out_path)?;

        let auth_alg = self.auth_alg.as_ref().map_or("HS256", |s| s);
        let database_url = self.database_url.as_ref().map_or("postgres://postgres:postgres@localhost:5432/postgres", |s| s);

        let final_config_string = DEFAULT_CONFIG.replace("{auth_alg}", auth_alg).replace("{database_default}", database_url);
        config_file.write_all(final_config_string.as_bytes())?;

        info!("Created justsql config file {:?}", config_out_path.as_os_str());

        Ok(())
    }
}
