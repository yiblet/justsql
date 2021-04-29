use clap::Clap;

mod run;
mod server;

/// This doc string acts as a help message when the user runs '--help'
/// as do all doc strings on fields
#[derive(Clap)]
#[clap(version = "0.0.1", author = "Shalom Yiblet <shalom.yiblet@gmail.com>")]
pub struct Opts {
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
    Run(run::Run),
    Server(server::Server),
}

pub trait Command {
    fn run_command(&self, opt: &Opts) -> anyhow::Result<()>;
}

impl Command for SubCommand {
    fn run_command(&self, opt: &Opts) -> anyhow::Result<()> {
        match self {
            SubCommand::Run(run) => run.run_command(opt),
            SubCommand::Server(server) => server.run_command(opt),
        }
    }
}
