use clap::Clap;

#[macro_use]
extern crate log;

#[macro_use]
extern crate anyhow;

mod binding;
mod codegen;
mod command;
mod config;
mod engine;
mod query;
mod row_type;
mod server;
mod util;

pub fn main() -> anyhow::Result<()> {
    env_logger::init_from_env(
        env_logger::Env::new().default_filter_or("actix_web=info,actix_server=info,justsql=info"),
    );

    if let Some(path) = dotenv::dotenv().ok() {
        info!("loaded .env file from {:?}", path.as_os_str())
    }
    let opt: command::Opts = command::Opts::parse();
    opt.run()
}
