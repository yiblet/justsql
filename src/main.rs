use anyhow::anyhow;
use std::{collections::BTreeMap, path::Path};

use clap::Clap;
mod parser;

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
    Debug(Debug),
}

trait Main {
    fn main(&self, opt: &Opts) -> anyhow::Result<()>;
}

/// Show the sql to be executed without running it
#[derive(Clap)]
struct Debug {
    /// location of the module file
    module: String,

    /// arguments to pass per module (in the form of arg_name=value)
    args: Vec<String>,
}

impl Main for Debug {
    fn main(&self, _opt: &Opts) -> anyhow::Result<()> {
        let args = self
            .args
            .iter()
            .map(|arg| parse_args(arg).ok_or_else(|| anyhow!("invalid arg {}", arg)))
            .collect::<anyhow::Result<BTreeMap<&str, &str>>>()?;

        let module = read_module(&self.module)?;
        if args.len() != module.params.len()
            || !module
                .params
                .iter()
                .all(|param| args.contains_key(param.as_str()))
        {
            Err(anyhow!("some argument does not exist in the sql"))?;
        }

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
                    print!("{}", arg)
                } else {
                    print!(", {}", arg)
                }
            }
            println!(");");
        }
        Ok(())
    }
}

fn parse_args(input: &str) -> Option<(&str, &str)> {
    let (prefix, suffix) = input.split_at(input.find('=')?);
    Some((prefix.trim(), &suffix[1..].trim()))
}

fn read_module<A: AsRef<Path>>(input: A) -> anyhow::Result<parser::Module> {
    use std::io::prelude::*;
    let path = input.as_ref();
    let mut file = std::fs::File::open(path)?;
    let mut file_content = String::with_capacity(file.metadata()?.len() as usize);
    file.read_to_string(&mut file_content)?;
    let (_, data) = parser::Module::parse(file_content.as_str())
        .map_err(|err| anyhow!("{}", err.to_string()))?;
    Ok(data)
}

pub fn main() -> anyhow::Result<()> {
    let opt: Opts = Opts::parse();

    match &opt.subcmd {
        SubCommand::Debug(debug) => debug.main(&opt),
    }
}
