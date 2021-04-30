use anyhow::anyhow;
use clap::Clap;

use crate::{
    args::{parse_args, Literal},
    ast::{Module, ParamType},
    row_type::convert_row,
};

use super::{Command, Opts};

/// run one single file
#[derive(Clap)]
pub struct Run {
    /// location of the module file
    module: String,

    /// arguments to pass per module (in the form of 'arg_name=value')
    args: Vec<String>,

    /// This prints the calls to database instead of actually connecting
    #[clap(short, long)]
    debug: bool,
}

impl Command for Run {
    // TODO split up this function
    fn run_command(&self, _opt: &Opts) -> anyhow::Result<()> {
        let args = parse_args(self.args.iter().map(String::as_str))?;

        let module = Module::from_path(&self.module)?;
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
                let (stmt, _) = statement.bind()?;
                for lines in stmt.split('\n').filter(|line| line.trim() != "") {
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
                        let (stmt, binding) = statement.bind()?;
                        let mut query = sqlx::query(stmt.as_str());

                        for literal in binding.into_iter().map(|param_type| match param_type {
                            ParamType::Param(param) => args
                                .get(param)
                                .ok_or_else(|| anyhow!("missing argument {}", param)),
                            ParamType::Auth(_) => {
                                Err(anyhow!("cannot use file that requires auth params with this justsql command"))
                            }
                        }) {
                            query = match literal? {
                                Literal::Int(i) => query.bind(i),
                                Literal::Float(f) => query.bind(f),
                                Literal::String(s) => query.bind(s),
                            };
                        }

                        let res: Vec<sqlx::postgres::PgRow> = query.fetch_all(&pool).await?;

                        for row in res {
                            let res = convert_row(row)?;
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
