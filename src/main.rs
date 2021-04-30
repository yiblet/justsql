use clap::Clap;

mod args;
mod ast;
mod binding;
mod command;
mod query;
mod row_type;
mod server;
mod util;
mod engine;

pub fn main() -> anyhow::Result<()> {
    env_logger::init_from_env(
        env_logger::Env::new().default_filter_or("actix_web=info,actix_server=info,justsql=info"),
    );
    let opt: command::Opts = command::Opts::parse();
    opt.run()
}
