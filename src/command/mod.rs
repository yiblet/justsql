use anyhow::Context;
use clap::Clap;
use serde::de::DeserializeOwned;

mod init;
mod peek;
mod print;
mod run;
mod server;

pub fn read_input<A: DeserializeOwned, B: DeserializeOwned>(
    input: &str,
    auth_input: Option<&str>,
) -> anyhow::Result<(A, Option<B>)> {
    let input: A = read_json_or_json_file(input).context("could not read input json")?;
    let auth_input: Option<B> = auth_input
        .map(|auth| read_json_or_json_file(auth).context("could not read input json"))
        .transpose()?;
    Ok((input, auth_input))
}

pub fn read_json_or_json_file<T: DeserializeOwned>(data: &str) -> anyhow::Result<T> {
    serde_json::from_str(data)
        .with_context(|| "input is not a json")
        .or_else(|_| -> anyhow::Result<_> {
            let file = std::fs::File::open(data)?;
            Ok(serde_json::from_reader(file)?)
        })
        .with_context(|| "input is not a json nor a readable json file path")
}

/// This doc string acts as a help message when the user runs '--help'
/// as do all doc strings on fields
#[derive(Clap)]
#[clap(version = "0.2.0", author = "Shalom Yiblet <shalom.yiblet@gmail.com>")]
pub struct Opts {
    /// Set the file path where justsql will read the configs from. If this is left unset,
    /// justsql will recursively look for a `justsql.config.yaml` in current and parent
    /// directories.
    #[clap(short, long)]
    config: Option<std::path::PathBuf>,
    #[clap(subcommand)]
    subcmd: SubCommand,
}

impl Opts {
    pub fn run(&self) -> anyhow::Result<()> {
        self.subcmd.run_command(self)
    }
}

#[derive(Clap)]
pub enum SubCommand {
    Init(init::Init),
    Peek(peek::Peek),
    Print(print::Print),
    Run(run::Run),
    Server(server::Server),
}

pub trait Command {
    fn run_command(&self, opt: &Opts) -> anyhow::Result<()>;
}

impl Command for SubCommand {
    fn run_command(&self, opt: &Opts) -> anyhow::Result<()> {
        match self {
            SubCommand::Init(init) => init.run_command(opt),
            SubCommand::Peek(peek) => peek.run_command(opt),
            SubCommand::Print(print) => print.run_command(opt),
            SubCommand::Run(run) => run.run_command(opt),
            SubCommand::Server(server) => server.run_command(opt),
        }
    }
}
