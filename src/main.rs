use anyhow::anyhow;
use args::{parse_args, Literal};
use std::{collections::BTreeMap, path::Path};

use clap::Clap;
use sqlx::{Column, ColumnIndex, Row};
mod args;
mod decorator;
mod module;
mod parser;
mod util;

/// This doc string acts as a help message when the user runs '--help'
/// as do all doc strings on fields
#[derive(Clap)]
#[clap(version = "0.0.1", author = "Shalom Yiblet <shalom.yiblet@gmail.com>")]
struct Opts {
    #[clap(subcommand)]
    subcmd: SubCommand,
}

#[derive(Clap)]
enum SubCommand {
    Run(Run),
}

trait Main {
    fn main(&self, opt: &Opts) -> anyhow::Result<()>;
}

/// Show the sql to be executed without running it
#[derive(Clap)]
struct Run {
    /// location of the module file
    module: String,

    /// arguments to pass per module (in the form of arg_name=value)
    args: Vec<String>,

    #[clap(long)]
    debug: bool,
}

impl Main for Run {
    fn main(&self, _opt: &Opts) -> anyhow::Result<()> {
        let args = parse_args(self.args.iter().map(String::as_str))?;

        let module = read_module(&self.module)?;
        if args.len() != module.params.len()
            || !module
                .params
                .iter()
                .all(|param| args.contains_key(param.as_str()))
        {
            Err(anyhow!("some argument does not exist in the sql"))?;
        }

        if self.debug {
            for statement in &module.sql {
                println!("PREPARE query AS");
                for lines in statement.split('\n').filter(|line| line.trim() != "") {
                    println!("    {}", lines);
                }
                println!(";");

                print!("EXECUTE query(");
                for (idx, arg) in module
                    .params
                    .iter()
                    .filter_map(|param| args.get(param.as_str()))
                    .enumerate()
                {
                    if idx == 0 {
                        print!("{}", arg.to_string())
                    } else {
                        print!(", {}", arg.to_string())
                    }
                }
                println!(");");
            }
        } else {
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap()
                .block_on(async {
                    let uri = crate::util::get_var("POSTGRES_URL")?;
                    let pool = sqlx::postgres::PgPoolOptions::new()
                        .max_connections(1)
                        .connect(uri.as_str())
                        .await?;

                    for statement in &module.sql {
                        let mut query = sqlx::query(statement.as_str());

                        for literal in module.bindings(&args) {
                            query = match literal? {
                                Literal::Int(i) => query.bind(i),
                                Literal::Float(f) => query.bind(f),
                                Literal::String(s) => query.bind(s),
                            };
                        }

                        let res: Vec<sqlx::postgres::PgRow> = query.fetch_all(&pool).await?;

                        for row in res {
                            let res: BTreeMap<&str, String> = row
                                .columns()
                                .iter()
                                .map(|col| -> anyhow::Result<_> {
                                    let name = col.name();
                                    let value: String =
                                        row.try_get(col.ordinal()).map_err(|err| {
                                            anyhow!(
                                                "could not get column {} due to {}",
                                                name,
                                                err.to_string()
                                            )
                                        })?;
                                    Ok((name, value))
                                })
                                .collect::<anyhow::Result<BTreeMap<_, _>>>()?;
                            let json = serde_json::to_string_pretty(&res)?;
                            println!("{}", json);
                        }
                    }

                    let res: anyhow::Result<()> = Ok(());
                    res
                })?;
        }

        Ok(())
    }
}

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
    let opt: Opts = Opts::parse();

    match &opt.subcmd {
        SubCommand::Run(runner) => runner.main(&opt),
    }
}
