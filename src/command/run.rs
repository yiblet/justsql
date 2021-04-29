use anyhow::anyhow;
use clap::Clap;

use crate::{
    args::{parse_args, Literal},
    ast::Module,
    row_type::convert_row,
};

use super::{Command, Opts};

/// run one module
#[derive(Clap)]
pub struct Run {
    /// location of the module file
    module: String,

    /// arguments to pass per module (in the form of arg_name=value)
    args: Vec<String>,

    #[clap(long)]
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
