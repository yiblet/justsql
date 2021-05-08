use std::collections::BTreeMap;

use anyhow::Context;
use clap::Clap;

use crate::{
    ast::{Interp, Module},
    binding::Binding,
};

use super::{read_json_or_json_file, Command, Opts};

/// display the query
#[derive(Clap)]
pub struct Print {
    /// location of the module file
    module: String,

    /// the payload as a json string or path to a file containing the payload
    json: Option<String>,

    /// the auth claims as a json string or path to a file containing the auth claims
    #[clap(short, long)]
    auth: Option<String>,
}

impl Command for Print {
    // TODO split up this function
    fn run_command(&self, _opt: &Opts) -> anyhow::Result<()> {
        let module = Module::from_path(&self.module).context("failed to find file")?;

        let payload = self
            .json
            .as_ref()
            .map(|payload| read_json_or_json_file::<BTreeMap<String, Binding>>(payload.as_str()))
            .transpose()?;

        let auth_claims = self
            .auth
            .as_ref()
            .map(|payload| read_json_or_json_file::<BTreeMap<String, Binding>>(payload.as_str()))
            .transpose()?
            .unwrap_or_default();

        for (idx, statement) in module.sql.iter().enumerate() {
            println!("PREPARE query_{} AS", idx);
            let (stmt, _) = statement.bind()?;
            for lines in stmt.split('\n').filter(|line| line.trim() != "") {
                println!("    {}", lines);
            }
            println!(";");

            if let Some(bindings) = payload.as_ref() {
                print!("EXECUTE query_{}(", idx);
                for (idx, arg) in statement
                    .0
                    .iter()
                    .filter_map(|interp| match interp {
                        Interp::Literal(_) => None,
                        Interp::Param(param) => {
                            Some(bindings.get(param.as_str()).ok_or_else(|| {
                                anyhow!("failed to find parameter {}", param.as_str())
                            }))
                        }
                        Interp::AuthParam(param) => {
                            Some(auth_claims.get(param.as_str()).ok_or_else(|| {
                                anyhow!("failed to find parameter {}", param.as_str())
                            }))
                        }
                    })
                    .enumerate()
                {
                    if idx == 0 {
                        print!("{}", arg?.to_sql_string()?)
                    } else {
                        print!(", {}", arg?.to_sql_string()?)
                    }
                }
                println!(");");
            }
        }

        Ok(())
    }
}
