use std::{collections::BTreeMap, path::Path};

use clap::Clap;

use crate::{
    binding::Binding,
    engine::{Importer, UpfrontImporter},
    query,
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
        let importer = UpfrontImporter::from_paths_or_print_error(&[self.module.as_ref()])
            .ok_or_else(|| anyhow!("importing sql failed"))?;
        let module = importer
            .get_module_from_location(Path::new(self.module.as_str()).canonicalize()?.as_path())?;

        let payload = self
            .json
            .as_ref()
            .map(|payload| read_json_or_json_file::<BTreeMap<String, Binding>>(payload.as_str()))
            .transpose()?;

        let auth_claims = self
            .auth
            .as_ref()
            .map(|payload| read_json_or_json_file::<BTreeMap<String, Binding>>(payload.as_str()))
            .transpose()?;

        for (idx, statement) in module.sql.iter().enumerate() {
            println!("PREPARE query_{} AS", idx);
            let (stmt, params) =
                query::build_query_statement(&module, &importer, statement.as_slice())?;
            for lines in stmt.split('\n').filter(|line| line.trim() != "") {
                println!("    {}", lines);
            }
            println!(";");

            if let Some(bindings) = payload.as_ref() {
                let bound_params =
                    query::bind_params(params.as_slice(), &bindings, auth_claims.as_ref())?;
                print!("EXECUTE query_{}(", idx);
                for (idx, arg) in bound_params.iter().cloned().enumerate() {
                    if idx == 0 {
                        print!("{}", arg.to_sql_string()?)
                    } else {
                        print!(", {}", arg.to_sql_string()?)
                    }
                }
                println!(");");
            }
        }

        Ok(())
    }
}
