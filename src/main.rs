use anyhow::anyhow;
use clap::Clap;
use std::path::Path;

mod args;
mod binding;
mod command;
mod decorator;
mod module;
mod parser;
mod query;
mod row_type;
mod util;
mod server;

fn read_module<A: AsRef<Path>>(input: A) -> anyhow::Result<module::Module> {
    use std::io::prelude::*;
    let path = input.as_ref();
    let mut file = std::fs::File::open(path)?;
    let mut file_content = String::with_capacity(file.metadata()?.len() as usize);
    file.read_to_string(&mut file_content)?;
    let (_, data) = module::Module::parse(file_content.as_str())
        .map_err(|err| anyhow!("{}", err.to_string()))?;
    Ok(data)
}

pub fn main() -> anyhow::Result<()> {
    env_logger::init_from_env(env_logger::Env::new().default_filter_or("actix_web=info,actix_server=info,justsql=info"));
    let opt: command::Opts = command::Opts::parse();
    opt.run()
}
